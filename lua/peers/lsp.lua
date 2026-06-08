local timing = require("peers.timing")

local M = {}

local PEERS_LSP_NAME = "peersdiff"
local LOOPBACK_HOST = "127.0.0.1"
local RENDER_METHOD = "peers/renderReview"
local CREATE_THREAD_METHOD = "peers/createThread"
local REPLY_TO_THREAD_METHOD = "peers/replyToThread"
local EDIT_COMMENT_METHOD = "peers/editComment"
local DELETE_COMMENT_METHOD = "peers/deleteComment"
local RESOLVE_THREAD_METHOD = "peers/resolveThread"
local REOPEN_THREAD_METHOD = "peers/reopenThread"
local TOGGLE_THREAD_COLLAPSED_METHOD = "peers/toggleThreadCollapsed"
local ASK_AGENT_METHOD = "peers/askAgent"
local REVIEW_UPDATED_NOTIFICATION = "peers/reviewUpdated"
local COMMAND_ADD_COMMENT = "peers.addComment"
local COMMAND_REPLY = "peers.reply"
local COMMAND_EDIT_COMMENT = "peers.editComment"
local COMMAND_DELETE_COMMENT = "peers.deleteComment"
local COMMAND_RESOLVE_THREAD = "peers.resolveThread"
local COMMAND_REOPEN_THREAD = "peers.reopenThread"
local COMMAND_TOGGLE_THREAD_COLLAPSED = "peers.toggleThreadCollapsed"
local COMMAND_RESPOND_TO_THREAD = "peers.respondToThread"
local INVALID_LSP_URL_ERROR = "Invalid nvim_lsp_url: "
local RENDER_READY_TIMEOUT = 5000
local RENDER_READY_INTERVAL = 50
local REFRESH_DEBOUNCE_MS = 75
local ATTACH_READY_TIMEOUT = 5000
local ATTACH_READY_INTERVAL = 100
local ATTACH_TIMEOUT_ERROR = "Peers LSP did not become ready"
local RENDER_TIMEOUT_ERROR = "Peers render request timed out"
local SOURCE_ATTACH_AUGROUP = "peers-source-lsp-attach"
local SOURCE_HELPER_BUFFER_VAR = "peers_source_helper"
local SOURCE_HELPER_SIGNATURE_VAR = "peers_source_signature"
local LSP_METHOD_CODE_ACTION = "textDocument/codeAction"

local SOURCE_SUPPRESSED_CAPABILITIES = {
  "hoverProvider",
  "definitionProvider",
  "referencesProvider",
  "documentSymbolProvider",
}

local pending_refreshes = {}
local source_sessions = {}
local source_attach_autocmds_created = false
local source_buffer_path

local function context_root(context)
  local client = context and context.client_id and vim.lsp.get_client_by_id(context.client_id) or nil
  return client and client.config and client.config.root_dir or nil
end

local function client_root(client)
  -- comment
  return client and client.config and client.config.root_dir or nil
end

local COMMAND_HANDLERS = {
  [COMMAND_ADD_COMMENT] = "comment_current",
  [COMMAND_REPLY] = "reply_to_thread",
  [COMMAND_EDIT_COMMENT] = "edit_comment",
  [COMMAND_DELETE_COMMENT] = "delete_comment",
  [COMMAND_RESOLVE_THREAD] = "resolve_thread",
  [COMMAND_REOPEN_THREAD] = "reopen_thread",
  [COMMAND_TOGGLE_THREAD_COLLAPSED] = "toggle_thread_collapsed",
  [COMMAND_RESPOND_TO_THREAD] = "respond_to_thread",
}

local MUTATION_METHODS = {
  create_thread = CREATE_THREAD_METHOD,
  reply_to_thread = REPLY_TO_THREAD_METHOD,
  edit_comment = EDIT_COMMENT_METHOD,
  delete_comment = DELETE_COMMENT_METHOD,
  resolve_thread = RESOLVE_THREAD_METHOD,
  reopen_thread = REOPEN_THREAD_METHOD,
  toggle_thread_collapsed = TOGGLE_THREAD_COLLAPSED_METHOD,
}

local function command_input(command)
  return command.arguments and command.arguments[1] or nil
end

local function build_command_handlers()
  local handlers = {}
  for command_name, handler_name in pairs(COMMAND_HANDLERS) do
    handlers[command_name] = function(command, context)
      if command_name == COMMAND_ADD_COMMENT then
        require("peers.buffer").comment_from_code_action(context and context.bufnr or nil, command_input(command), {
          client_id = context and context.client_id or nil,
        })
      else
        require("peers.buffer")[handler_name](context and context.bufnr or nil, command_input(command))
      end
    end
  end
  return handlers
end

local function review_updated_handler(_, _, context)
  if not context or not context.client_id then
    return
  end
  local root = context_root(context)
  if pending_refreshes[context.client_id] then
    timing.log(root, "lsp", "reviewUpdated coalesced client=" .. tostring(context.client_id))
    return
  end

  timing.log(root, "lsp", "reviewUpdated scheduled client=" .. tostring(context.client_id))
  pending_refreshes[context.client_id] = true
  vim.defer_fn(function()
    pending_refreshes[context.client_id] = nil
    timing.log(root, "lsp", "reviewUpdated firing client=" .. tostring(context.client_id))
    require("peers.buffer").refresh_from_client(context.client_id)
  end, REFRESH_DEBOUNCE_MS)
end

local function install_handlers()
  vim.lsp.handlers[REVIEW_UPDATED_NOTIFICATION] = review_updated_handler
end

local function lsp_port(session)
  local port = tostring(session.nvim_lsp_url or ""):match(":(%d+)$")
  if not port then
    error(INVALID_LSP_URL_ERROR .. tostring(session.nvim_lsp_url))
  end
  return tonumber(port)
end

local function lsp_clients()
  if vim.lsp.get_clients then
    return vim.lsp.get_clients({ name = PEERS_LSP_NAME })
  end
  return vim.lsp.get_active_clients({ name = PEERS_LSP_NAME })
end

local function lsp_client_for_port(port)
  for _, client in ipairs(lsp_clients()) do
    if client.config and client.config.peers_port == port then
      return client
    end
  end
  return nil
end

local function buffer_has_client(buf, client_id)
  local clients = vim.lsp.get_clients and vim.lsp.get_clients({ bufnr = buf }) or vim.lsp.get_active_clients({ bufnr = buf })
  for _, client in ipairs(clients) do
    if client.id == client_id then
      return true
    end
  end
  return false
end

local function method_name(method)
  return type(method) == "string" and method or tostring(method)
end

local function method_bufnr(opts)
  if type(opts) == "number" then
    return opts
  end
  if type(opts) == "table" then
    return opts.bufnr
  end
  return nil
end

local function source_buffer_supports_method(method)
  return method_name(method) == LSP_METHOD_CODE_ACTION
end

local function configure_source_method_filter(client)
  if not client or client.config.peers_source_method_filter then
    return
  end
  client.config.peers_source_method_filter = true
  local supports_method = client.supports_method
  if type(supports_method) ~= "function" then
    return
  end

  client.supports_method = function(self, method, opts)
    local bufnr = method_bufnr(opts)
    if bufnr and source_buffer_path(bufnr) then
      return source_buffer_supports_method(method)
    end
    return supports_method(self, method, opts)
  end
end

local function attach_existing_client(buf, client, opts)
  opts = opts or {}
  if not opts.source then
    return vim.lsp.buf_attach_client(buf, client.id)
  end

  local capabilities = client.server_capabilities or {}
  local suppressed = {}
  for _, capability in ipairs(SOURCE_SUPPRESSED_CAPABILITIES) do
    suppressed[capability] = capabilities[capability]
    capabilities[capability] = nil
  end
  local attached = vim.lsp.buf_attach_client(buf, client.id)
  vim.schedule(function()
    if not client.server_capabilities then
      return
    end
    for capability, value in pairs(suppressed) do
      if client.server_capabilities[capability] == nil then
        client.server_capabilities[capability] = value
      end
    end
  end)
  return attached
end

function M.stop_stale_clients(port)
  for _, client in ipairs(lsp_clients()) do
    local config = client.config or {}
    if config.peers_port ~= port then
      client.stop(true)
    end
  end
end

function M.attach(buf, root, session, opts)
  opts = opts or {}
  local port = lsp_port(session)
  install_handlers()
  M.stop_stale_clients(port)

  local client = lsp_client_for_port(port)
  if client then
    configure_source_method_filter(client)
    if buffer_has_client(buf, client.id) then
      return client.id
    end
    if attach_existing_client(buf, client, opts) then
      return client.id
    end
  end

  local client_id = vim.lsp.start({
    name = PEERS_LSP_NAME,
    cmd = vim.lsp.rpc.connect(LOOPBACK_HOST, port),
    root_dir = root,
    peers_port = port,
    commands = build_command_handlers(),
    handlers = {
      [REVIEW_UPDATED_NOTIFICATION] = review_updated_handler,
    },
  }, {
    bufnr = buf,
    reuse_client = function(client, config)
      return client.name == PEERS_LSP_NAME and client.config.peers_port == config.peers_port
    end,
  })
  if client_id then
    configure_source_method_filter(vim.lsp.get_client_by_id(client_id))
  end
  return client_id
end

local function normalized_path(path)
  if vim.fs and vim.fs.normalize then
    return vim.fs.normalize(path)
  end
  return path
end

local function path_under_root(path, root)
  path = normalized_path(path)
  root = normalized_path(root)
  return path == root or vim.startswith(path, root .. "/")
end

source_buffer_path = function(buf)
  if not vim.api.nvim_buf_is_valid(buf) then
    return nil
  end
  if not vim.api.nvim_buf_is_loaded(buf) then
    return nil
  end
  if vim.bo[buf].buftype ~= "" then
    return nil
  end
  if not vim.bo[buf].buflisted then
    return nil
  end
  local name = vim.api.nvim_buf_get_name(buf)
  if name == "" or vim.startswith(name, "peers://") then
    return nil
  end
  if vim.b[buf][SOURCE_HELPER_BUFFER_VAR] and not vim.bo[buf].buflisted then
    return nil
  end
  if vim.b[buf][SOURCE_HELPER_BUFFER_VAR] then
    vim.b[buf][SOURCE_HELPER_BUFFER_VAR] = nil
    vim.b[buf][SOURCE_HELPER_SIGNATURE_VAR] = nil
  end
  return name
end

local function attach_source_buffer(buf, root, session)
  local path = source_buffer_path(buf)
  if not path or not path_under_root(path, root) then
    return
  end
  local relative = normalized_path(path):sub(#normalized_path(root) + 2)
  if vim.startswith(relative, ".git/") then
    return
  end
  local client_id = M.attach(buf, root, session, { source = true })
  if client_id then
    require("peers.buffer").apply_source_decorations_for_source(buf, root)
    timing.log(root, "lsp", "attached source buffer client=" .. tostring(client_id) .. " buf=" .. tostring(buf) .. " path=" .. relative)
  end
end

local function attach_known_source_buffers(root, session)
  for _, buf in ipairs(vim.api.nvim_list_bufs()) do
    attach_source_buffer(buf, root, session)
  end
end

local function schedule_source_buffer_attach(buf)
  vim.schedule(function()
    if not vim.api.nvim_buf_is_valid(buf) then
      return
    end
    for root, session in pairs(source_sessions) do
      attach_source_buffer(buf, root, session)
    end
  end)
end

local function ensure_source_attach_autocmds()
  if source_attach_autocmds_created then
    return
  end
  source_attach_autocmds_created = true
  vim.api.nvim_create_autocmd({ "BufReadPost", "BufNewFile", "BufEnter", "BufWinEnter", "FileType" }, {
    group = vim.api.nvim_create_augroup(SOURCE_ATTACH_AUGROUP, { clear = true }),
    callback = function(event)
      schedule_source_buffer_attach(event.buf)
    end,
  })
end

function M.attach_repo_sources(root, session)
  source_sessions[normalized_path(root)] = session
  ensure_source_attach_autocmds()
  attach_known_source_buffers(root, session)
end

function M.attach_when_ready(buf, root, session, on_ready)
  local remaining = math.max(1, math.floor(ATTACH_READY_TIMEOUT / ATTACH_READY_INTERVAL))

  local function poll()
    local client_id = M.attach(buf, root, session)
    local client = client_id and vim.lsp.get_client_by_id(client_id) or nil
    if client and client.initialized then
      on_ready(client_id)
      return
    end

    remaining = remaining - 1
    if remaining <= 0 then
      vim.notify(ATTACH_TIMEOUT_ERROR, vim.log.levels.ERROR)
      return
    end

    vim.defer_fn(poll, ATTACH_READY_INTERVAL)
  end

  poll()
end

local function request_render(client, buf, on_render)
  local start = timing.now()
  client:request(RENDER_METHOD, nil, function(error, result)
    timing.log(client_root(client), "lsp", string.format("render rpc callback %.1fms buf=%s", timing.ms(start), tostring(buf)))
    if error then
      vim.notify(error.message or tostring(error), vim.log.levels.ERROR)
      on_render(nil)
      return
    end
    on_render(result)
  end, buf)
end

function M.render(client_id, buf, on_render)
  local remaining = math.max(1, math.floor(RENDER_READY_TIMEOUT / RENDER_READY_INTERVAL))

  local function poll()
    local client = vim.lsp.get_client_by_id(client_id)
    if not client then
      remaining = remaining - 1
      if remaining <= 0 then
        vim.notify(RENDER_TIMEOUT_ERROR, vim.log.levels.ERROR)
        return
      end
      vim.defer_fn(poll, RENDER_READY_INTERVAL)
      return
    end
    if client.initialized then
      request_render(client, buf, on_render)
      return
    end

    remaining = remaining - 1
    if remaining <= 0 then
      return
    end
    vim.defer_fn(poll, RENDER_READY_INTERVAL)
  end

  poll()
end

function M.render_now(client_id, buf, on_render)
  local client = vim.lsp.get_client_by_id(client_id)
  if not client then
    return false
  end

  request_render(client, buf, on_render)
  return true
end

local function mutate(client_id, buf, method, request, on_render)
  local client = vim.lsp.get_client_by_id(client_id)
  if not client then
    return
  end

  client:request(method, request, function(error, result)
    if error then
      vim.notify(error.message or tostring(error), vim.log.levels.ERROR)
      return
    end
    on_render(result)
  end, buf)
end

for function_name, method in pairs(MUTATION_METHODS) do
  M[function_name] = function(client_id, buf, request, on_render)
    mutate(client_id, buf, method, request, on_render)
  end
end

function M.ask_agent(client_id, buf, request, on_done)
  local client = vim.lsp.get_client_by_id(client_id)
  if not client then
    return
  end

  client:request(ASK_AGENT_METHOD, request, function(error, result)
    if error then
      vim.notify(error.message or tostring(error), vim.log.levels.ERROR)
      return
    end
    if on_done then
      on_done(result)
    end
  end, buf)
end

return M
