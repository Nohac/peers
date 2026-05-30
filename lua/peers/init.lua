local buffer = require("peers.buffer")
local lsp_proxy = require("peers.lsp_proxy")
local session = require("peers.session")

local M = {}

local AUGROUP = "peers-nvim-session"
local COMMAND_REVIEW = "PeersReview"
local COMMAND_COMMENT = "PeersComment"
local COMMAND_STOP = "PeersStop"

local defaults = {
  binary = "peers",
  start_timeout_ms = 10000,
  poll_interval_ms = 100,
  stop_on_exit = true,
}

local config = vim.deepcopy(defaults)
local configured = false

local function define_commands()
  vim.api.nvim_create_user_command(COMMAND_REVIEW, function(command)
    M.review({ review = command.args ~= "" and command.args or nil })
  end, {
    nargs = "?",
    complete = "file",
  })

  vim.api.nvim_create_user_command(COMMAND_STOP, function()
    session.stop()
  end, {})

  vim.api.nvim_create_user_command(COMMAND_COMMENT, function()
    buffer.comment_current()
  end, {})
end

local function define_autocmds()
  vim.api.nvim_create_autocmd("VimLeavePre", {
    group = vim.api.nvim_create_augroup(AUGROUP, { clear = true }),
    callback = function()
      if config.stop_on_exit and session.started_by_nvim() then
        session.stop()
      end
    end,
  })
end

function M.setup(opts)
  if opts ~= nil or not configured then
    config = vim.tbl_deep_extend("force", vim.deepcopy(defaults), opts or {})
    configured = true
  end

  define_commands()
  define_autocmds()
  lsp_proxy.setup()
end

function M.review(opts)
  opts = opts or {}
  local root = session.repo_root()
  local review_id = opts.review or session.current_review_id(root)
  local active = session.read_live_session(root, review_id)

  if active then
    buffer.open(root, review_id, active)
    return
  end

  session.start(config, root, review_id)
  session.wait_for_session(config, root, review_id, function(started)
    buffer.open(root, review_id, started)
  end)
end

return M
