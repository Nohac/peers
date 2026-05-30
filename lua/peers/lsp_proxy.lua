local buffer = require("peers.buffer")

local M = {}

local METHOD_HOVER = "textDocument/hover"
local METHOD_DECLARATION = "textDocument/declaration"
local METHOD_DEFINITION = "textDocument/definition"
local METHOD_TYPE_DEFINITION = "textDocument/typeDefinition"
local METHOD_IMPLEMENTATION = "textDocument/implementation"
local METHOD_REFERENCES = "textDocument/references"
local HOVER_FOCUS_ID = METHOD_HOVER
local HOVER_SEPARATOR = "---"
local HOVER_CLIENT_TITLE = "# %s"
local HOVER_PLAINTEXT_FENCE = "```"
local LOCATIONS_TITLE = "LSP locations"
local REFERENCES_TITLE = "References"
local NO_SOURCE_CLIENT_MESSAGE = "No source LSP client attached for "
local NO_HOVER_MESSAGE = "No information available"
local EMPTY_HOVER_MESSAGE = "Empty hover response"
local NO_LOCATIONS_MESSAGE = "No locations found"
local NO_REFERENCES_MESSAGE = "No references found"
local LSP_ATTACH_RETRY_MS = 120
local DEFAULT_REUSE_WIN = true
local installed = false
local original = {}

local function clients_for(buf, method)
  if vim.lsp.get_clients then
    return vim.lsp.get_clients({ bufnr = buf, method = method })
  end
  local clients = vim.lsp.get_active_clients({ bufnr = buf })
  return vim.tbl_filter(function(client)
    if client.supports_method then
      return client:supports_method(method, { bufnr = buf })
    end
    return true
  end, clients)
end

local function source_location()
  local review_buf = vim.api.nvim_get_current_buf()
  if not buffer.is_review_buffer(review_buf) then
    return nil
  end
  local source = buffer.source_location(review_buf)
  return source
end

local function position_params(source, client)
  return {
    textDocument = {
      uri = vim.uri_from_bufnr(source.bufnr),
    },
    position = {
      line = source.row,
      character = vim.lsp.util.character_offset(source.bufnr, source.row, source.col, client.offset_encoding),
    },
  }
end

local function request_clients(source, method, params_for_client, callback, retried)
  local clients = clients_for(source.bufnr, method)
  if #clients == 0 and not retried then
    vim.defer_fn(function()
      request_clients(source, method, params_for_client, callback, true)
    end, LSP_ATTACH_RETRY_MS)
    return
  end

  if #clients == 0 then
    vim.notify(NO_SOURCE_CLIENT_MESSAGE .. tostring(source.path), vim.log.levels.WARN)
    return
  end

  local pending = #clients
  local results = {}
  for _, client in ipairs(clients) do
    local params = params_for_client(client)
    client:request(method, params, function(error, result)
      results[client.id] = {
        err = error,
        result = result,
      }
      pending = pending - 1
      if pending == 0 then
        callback(results)
      end
    end, source.bufnr)
  end
end

local function valid_hover_contents(contents)
  if type(contents) == "string" then
    return #contents > 0
  end
  if type(contents) ~= "table" then
    return false
  end
  local value = vim.tbl_get(contents, "value")
    or vim.tbl_get(contents, 1, "value")
    or contents[1]
    or ""
  return #value > 0
end

local function append_hover_contents(contents, result, client_name, multiple)
  local markup_kind = vim.lsp.protocol.MarkupKind
  if multiple then
    table.insert(contents, string.format(HOVER_CLIENT_TITLE, client_name))
  end

  if type(result.contents) == "table" and result.contents.kind == markup_kind.PlainText then
    if multiple then
      table.insert(contents, HOVER_PLAINTEXT_FENCE)
      vim.list_extend(contents, vim.split(result.contents.value or "", "\n", { trimempty = true }))
      table.insert(contents, HOVER_PLAINTEXT_FENCE)
    else
      return vim.split(result.contents.value or "", "\n", { trimempty = true }), markup_kind.PlainText
    end
  else
    vim.list_extend(contents, vim.lsp.util.convert_input_to_markdown_lines(result.contents))
  end

  table.insert(contents, HOVER_SEPARATOR)
  return contents, markup_kind.Markdown
end

local function proxy_hover(config)
  local source = source_location()
  if not source then
    return false
  end

  config = config or {}
  config.focus_id = HOVER_FOCUS_ID
  request_clients(source, METHOD_HOVER, function(client)
    return position_params(source, client)
  end, function(results)
    local hovers = {}
    local empty_response = false
    for client_id, response in pairs(results) do
      local client = vim.lsp.get_client_by_id(client_id)
      if response.err then
        vim.lsp.log.error(response.err.code, response.err.message)
      elseif response.result and valid_hover_contents(response.result.contents) then
        table.insert(hovers, {
          client = client,
          result = response.result,
        })
      elseif response.result then
        empty_response = true
      end
    end

    if #hovers == 0 then
      if config.silent ~= true then
        vim.notify(empty_response and EMPTY_HOVER_MESSAGE or NO_HOVER_MESSAGE, vim.log.levels.INFO)
      end
      return
    end

    local contents = {}
    local format = vim.lsp.protocol.MarkupKind.Markdown
    for _, hover in ipairs(hovers) do
      local next_contents, next_format =
        append_hover_contents(contents, hover.result, hover.client and hover.client.name or "", #hovers > 1)
      contents = next_contents
      format = next_format
    end
    if contents[#contents] == HOVER_SEPARATOR then
      contents[#contents] = nil
    end

    vim.lsp.util.open_floating_preview(contents, format, config)
  end)
  return true
end

local function location_results(results)
  local raw_locations = {}
  local items = {}
  for client_id, response in pairs(results) do
    local client = vim.lsp.get_client_by_id(client_id)
    local result = response.result
    local locations = {}
    if result then
      locations = vim.islist(result) and result or { result }
    end
    for _, location in ipairs(locations) do
      table.insert(raw_locations, {
        client = client,
        location = location,
      })
    end
    if client then
      vim.list_extend(items, vim.lsp.util.locations_to_items(locations, client.offset_encoding))
    end
  end
  return raw_locations, items
end

local function jump_to_location(location, client, opts)
  opts = opts or {}
  if vim.lsp.util.show_document then
    vim.lsp.util.show_document(location, client.offset_encoding, {
      focus = true,
      reuse_win = opts.reuse_win ~= false,
    })
    return
  end
  vim.lsp.util.jump_to_location(location, client.offset_encoding, opts.reuse_win ~= false)
end

local function set_location_list(title, items, method, opts, source)
  local list = {
    title = title,
    items = items,
    context = {
      bufnr = source.bufnr,
      method = method,
    },
  }

  if opts.on_list then
    opts.on_list(list)
  elseif opts.loclist then
    vim.fn.setloclist(0, {}, " ", list)
    vim.cmd.lopen()
  else
    vim.fn.setqflist({}, " ", list)
    vim.cmd("botright copen")
  end
end

local function proxy_locations(method, opts, title, empty_message, extra_params)
  local source = source_location()
  if not source then
    return false
  end

  opts = opts or {}
  request_clients(source, method, function(client)
    return vim.tbl_extend("force", position_params(source, client), extra_params or {})
  end, function(results)
    local raw_locations, items = location_results(results)
    if #items == 0 then
      vim.notify(empty_message, vim.log.levels.INFO)
      return
    end
    if #raw_locations == 1 and not opts.on_list then
      jump_to_location(raw_locations[1].location, raw_locations[1].client, {
        reuse_win = opts.reuse_win == nil and DEFAULT_REUSE_WIN or opts.reuse_win,
      })
      return
    end
    set_location_list(title, items, method, opts, source)
  end)
  return true
end

local function proxy_references(context, opts)
  return proxy_locations(METHOD_REFERENCES, opts, REFERENCES_TITLE, NO_REFERENCES_MESSAGE, {
    context = context or {
      includeDeclaration = true,
    },
  })
end

local function wrap(name, replacement)
  original[name] = vim.lsp.buf[name]
  vim.lsp.buf[name] = replacement
end

function M.setup()
  if installed then
    return
  end

  installed = true
  wrap("hover", function(config)
    if proxy_hover(config) then
      return
    end
    return original.hover(config)
  end)
  wrap("declaration", function(opts)
    if proxy_locations(METHOD_DECLARATION, opts, LOCATIONS_TITLE, NO_LOCATIONS_MESSAGE) then
      return
    end
    return original.declaration(opts)
  end)
  wrap("definition", function(opts)
    if proxy_locations(METHOD_DEFINITION, opts, LOCATIONS_TITLE, NO_LOCATIONS_MESSAGE) then
      return
    end
    return original.definition(opts)
  end)
  wrap("type_definition", function(opts)
    if proxy_locations(METHOD_TYPE_DEFINITION, opts, LOCATIONS_TITLE, NO_LOCATIONS_MESSAGE) then
      return
    end
    return original.type_definition(opts)
  end)
  wrap("implementation", function(opts)
    if proxy_locations(METHOD_IMPLEMENTATION, opts, LOCATIONS_TITLE, NO_LOCATIONS_MESSAGE) then
      return
    end
    return original.implementation(opts)
  end)
  wrap("references", function(context, opts)
    if proxy_references(context, opts) then
      return
    end
    return original.references(context, opts)
  end)
end

return M
