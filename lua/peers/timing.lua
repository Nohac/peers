local M = {}

local LOG_FILE_NAME = "nvim.log"

function M.enabled()
  return vim.env.PEERS_TIMING and vim.env.PEERS_TIMING ~= "0"
end

function M.now()
  return vim.uv.hrtime()
end

function M.ms(start)
  return (M.now() - start) / 1000000
end

local function log_path(root)
  if root and root ~= "" then
    return root .. "/.peers/" .. LOG_FILE_NAME
  end
  return vim.fn.stdpath("cache") .. "/peers-nvim.log"
end

function M.log(root, scope, message)
  if not M.enabled() then
    return
  end

  local path = log_path(root)
  local parent = vim.fn.fnamemodify(path, ":h")
  if parent and parent ~= "" then
    vim.fn.mkdir(parent, "p")
  end

  local line = string.format(
    "%s peers timing lua %s: %s",
    os.date("!%Y-%m-%dT%H:%M:%SZ"),
    scope,
    message
  )
  vim.fn.writefile({ line }, path, "a")
end

return M
