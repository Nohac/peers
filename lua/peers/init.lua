local buffer = require("peers.buffer")
local lsp_proxy = require("peers.lsp_proxy")
local session = require("peers.session")

local M = {}

local AUGROUP = "peers-nvim-session"
local COMMAND = "Peers"
local COMMAND_REVIEW = "PeersReview"
local COMMAND_COMMENT = "PeersComment"
local COMMAND_AGENT = "PeersAgent"
local COMMAND_STOP = "PeersStop"
local SUBCOMMAND_DIFF = "diff"
local SUBCOMMAND_REVIEW = "review"
local SUBCOMMAND_OPEN = "open"
local SUBCOMMAND_STOP = "stop"
local DIFF_MODE_CACHED = "cached"
local DIFF_MODE_ALL = "all"
local DEFAULT_REVIEW_BASE = "main"
local DEFAULT_REVIEW_HEAD = "HEAD"
local UNKNOWN_COMMAND_ERROR = "Unknown Peers command"

local defaults = {
  binary = "peers",
  start_timeout_ms = 10000,
  poll_interval_ms = 100,
  stop_on_exit = true,
}

local config = vim.deepcopy(defaults)
local configured = false

local function split_args(input)
  return vim.split(vim.trim(input or ""), "%s+", { trimempty = true })
end

local function peers_complete(arg_lead, command_line)
  local args = split_args(command_line)
  if #args <= 2 then
    return vim.tbl_filter(function(item)
      return vim.startswith(item, arg_lead)
    end, { SUBCOMMAND_DIFF, SUBCOMMAND_REVIEW, SUBCOMMAND_OPEN, SUBCOMMAND_STOP })
  end

  if args[2] == SUBCOMMAND_DIFF then
    return vim.tbl_filter(function(item)
      return vim.startswith(item, arg_lead)
    end, { DIFF_MODE_CACHED, DIFF_MODE_ALL })
  end

  return {}
end

local function define_commands()
  vim.api.nvim_create_user_command(COMMAND, function(command)
    M.command(command.args)
  end, {
    nargs = "*",
    complete = peers_complete,
  })

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

  vim.api.nvim_create_user_command(COMMAND_AGENT, function(command)
    buffer.ask_agent(nil, command.args)
  end, {
    nargs = "+",
  })
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
  local review_id = "repo"
  local active = session.read_live_session(root, review_id)

  if active then
    buffer.open(root, review_id, active)
    return
  end

  session.start(config, root, {
    mode = SUBCOMMAND_DIFF,
  })
  session.wait_for_session(config, root, review_id, function(started)
    buffer.open(root, review_id, started)
  end)
end

function M.diff(opts)
  opts = opts or {}
  local root = session.repo_root()
  session.start(config, root, {
    mode = SUBCOMMAND_DIFF,
    cached = opts.cached == true,
    all = opts.all == true,
  })
  session.wait_for_current_session(config, root, function(review_id, started)
    buffer.open(root, review_id, started)
  end)
end

function M.branch_review(opts)
  opts = opts or {}
  local root = session.repo_root()
  session.start(config, root, {
    mode = SUBCOMMAND_REVIEW,
    base = opts.base or DEFAULT_REVIEW_BASE,
    head = opts.head or DEFAULT_REVIEW_HEAD,
  })
  session.wait_for_current_session(config, root, function(review_id, started)
    buffer.open(root, review_id, started)
  end)
end

function M.command(input)
  local args = split_args(input)
  local subcommand = args[1] or SUBCOMMAND_DIFF

  if subcommand == SUBCOMMAND_DIFF then
    local mode = args[2]
    M.diff({
      cached = mode == DIFF_MODE_CACHED,
      all = mode == DIFF_MODE_ALL,
    })
    return
  end

  if subcommand == SUBCOMMAND_REVIEW then
    M.branch_review({
      base = args[2],
      head = args[3],
    })
    return
  end

  if subcommand == SUBCOMMAND_OPEN then
    M.review({ review = args[2] })
    return
  end

  if subcommand == SUBCOMMAND_STOP then
    session.stop()
    return
  end

  error(UNKNOWN_COMMAND_ERROR .. ": " .. tostring(subcommand))
end

return M
