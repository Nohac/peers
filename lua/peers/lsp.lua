local M = {}

local PEERS_LSP_NAME = "peersdiff"
local LOOPBACK_HOST = "127.0.0.1"

local function lsp_port(session)
  local port = tostring(session.nvim_lsp_url or ""):match(":(%d+)$")
  if not port then
    error("Invalid nvim_lsp_url: " .. tostring(session.nvim_lsp_url))
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

  vim.lsp.start({
    name = PEERS_LSP_NAME,
    cmd = vim.lsp.rpc.connect(LOOPBACK_HOST, port),
    root_dir = root,
    peers_port = port,
  }, {
    bufnr = buf,
    reuse_client = function(client, config)
      return client.name == PEERS_LSP_NAME and client.config.peers_port == config.peers_port
    end,
  })
end

return M
