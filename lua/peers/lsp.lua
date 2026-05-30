local M = {}

local PEERS_LSP_NAME = "peersdiff"
local LOOPBACK_HOST = "127.0.0.1"
local RENDER_METHOD = "peers/renderReview"
local CREATE_THREAD_METHOD = "peers/createThread"
local COMMAND_ADD_COMMENT = "peers.addComment"
local INVALID_LSP_URL_ERROR = "Invalid nvim_lsp_url: "
local RENDER_READY_TIMEOUT = 5000
local RENDER_READY_INTERVAL = 50

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

function M.stop_stale_clients(port)
  for _, client in ipairs(lsp_clients()) do
    local config = client.config or {}
    if config.peers_port ~= port then
      client.stop(true)
    end
  end
end

function M.attach(buf, root, session)
  local port = lsp_port(session)
  M.stop_stale_clients(port)

  return vim.lsp.start({
    name = PEERS_LSP_NAME,
    cmd = vim.lsp.rpc.connect(LOOPBACK_HOST, port),
    root_dir = root,
    peers_port = port,
    commands = {
      [COMMAND_ADD_COMMENT] = function(command, context)
        local anchor = command.arguments and command.arguments[1] or nil
        require("peers.buffer").comment_current(context and context.bufnr or nil, anchor)
      end,
    },
  }, {
    bufnr = buf,
    reuse_client = function(client, config)
      return client.name == PEERS_LSP_NAME and client.config.peers_port == config.peers_port
    end,
  })
end

local function request_render(client, buf, on_render)
  client:request(RENDER_METHOD, nil, function(error, result)
    if error then
      vim.notify(error.message or tostring(error), vim.log.levels.ERROR)
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
    return
  end

  request_render(client, buf, on_render)
end

function M.create_thread(client_id, buf, request, on_render)
  local client = vim.lsp.get_client_by_id(client_id)
  if not client then
    return
  end

  client:request(CREATE_THREAD_METHOD, request, function(error, result)
    if error then
      vim.notify(error.message or tostring(error), vim.log.levels.ERROR)
      return
    end
    on_render(result)
  end, buf)
end

return M
