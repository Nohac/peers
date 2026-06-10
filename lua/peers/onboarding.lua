local M = {}

local BUFFER_NAME = "peers://onboarding/install"
local FILETYPE = "peershelp"
local NAMESPACE = vim.api.nvim_create_namespace("peers-onboarding")
local HIGHLIGHT_TITLE = "PeersOnboardingTitle"
local HIGHLIGHT_TEXT = "PeersOnboardingText"
local HIGHLIGHT_CODE = "PeersOnboardingCode"
local HIGHLIGHT_MUTED = "PeersOnboardingMuted"

local function define_highlights()
  pcall(vim.api.nvim_set_hl, 0, HIGHLIGHT_TITLE, { default = true, link = "Title" })
  pcall(vim.api.nvim_set_hl, 0, HIGHLIGHT_TEXT, { default = true, link = "Normal" })
  pcall(vim.api.nvim_set_hl, 0, HIGHLIGHT_CODE, { default = true, link = "String" })
  pcall(vim.api.nvim_set_hl, 0, HIGHLIGHT_MUTED, { default = true, link = "Comment" })
end

local function append_lines(lines, section)
  vim.list_extend(lines, section)
end

local function cli_install_lines(binary)
  local name = binary or "peers"
  return {
    "Install the CLI:",
    "",
    "  cargo install --git https://github.com/Nohac/peers",
    "",
    "Requirements:",
    "",
    "  - Rust and Cargo are installed",
    "  - Cargo's bin directory is on PATH, usually ~/.cargo/bin",
    "  - The configured Peers binary is executable: " .. name,
  }
end

local function session_lines()
  return {
    "Start a review session:",
    "",
    "  :Peers diff          Review unstaged changes",
    "  :Peers diff cached   Review staged changes",
    "  :Peers diff all      Review staged and unstaged changes",
    "  :Peers review        Review HEAD against main",
    "  :Peers review main   Review HEAD against main",
    "  :Peers review main HEAD",
  }
end

local function shortcut_lines()
  return {
    "Useful shortcuts:",
    "",
    "  c      comment or reply",
    "  v + c  comment selected lines",
    "  r      resolve or reopen thread",
    "  x      collapse or expand thread",
    "  X      collapse or expand file",
    "  D / U  next / previous thread",
    "  i      files sidebar",
    "  o      comments sidebar",
    "  p      return to review buffer",
    "  S      ask agent to commit",
    "  A      ask agent to review open threads",
    "  R      ask agent to fix and resolve thread",
    "  C      ask agent to comment without code changes",
  }
end

local function close_hint_lines()
  return {
    "Press q to close this buffer.",
  }
end

local function missing_binary_lines(binary)
  local lines = {
    "Peers is not installed",
    "",
  }
  append_lines(lines, cli_install_lines(binary))
  append_lines(lines, { "" })
  append_lines(lines, session_lines())
  append_lines(lines, { "" })
  append_lines(lines, shortcut_lines())
  append_lines(lines, { "" })
  append_lines(lines, close_hint_lines())
  return lines
end

local function highlight_lines(buf, lines)
  vim.api.nvim_buf_clear_namespace(buf, NAMESPACE, 0, -1)
  for index, line in ipairs(lines) do
    local row = index - 1
    local group = HIGHLIGHT_TEXT
    if line == "Peers is not installed" then
      group = HIGHLIGHT_TITLE
    elseif line:match("^  :") or line:match("^  cargo ") then
      group = HIGHLIGHT_CODE
    elseif line:match("^  %-") or line == "Press q to close this buffer." then
      group = HIGHLIGHT_MUTED
    end
    vim.api.nvim_buf_set_extmark(buf, NAMESPACE, row, 0, {
      end_col = #line,
      hl_group = group,
    })
  end
end

local function configure_buffer(buf)
  vim.bo[buf].buftype = "nofile"
  vim.bo[buf].bufhidden = "wipe"
  vim.bo[buf].buflisted = false
  vim.bo[buf].swapfile = false
  vim.bo[buf].filetype = FILETYPE
  vim.bo[buf].modifiable = false
  vim.keymap.set("n", "q", function()
    pcall(vim.api.nvim_buf_delete, buf, { force = true })
  end, { buffer = buf, desc = "Close Peers onboarding", nowait = true })
end

function M.open_missing_binary(binary)
  define_highlights()
  local buf = vim.api.nvim_create_buf(false, true)
  local lines = missing_binary_lines(binary)
  vim.api.nvim_buf_set_name(buf, BUFFER_NAME)
  vim.bo[buf].modifiable = true
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
  configure_buffer(buf)
  highlight_lines(buf, lines)
  vim.api.nvim_set_current_buf(buf)
end

return M
