local M = {}

local BUFFER_PREFIX = "peers://review-sidebar/"
local FILETYPE = "peerssidebar"
local MODE_FILES = "files"
local MODE_COMMENTS = "comments"
local WIDTH = 36
local MIN_REVIEW_WIDTH = 90
local ROW_KIND_FILE_HEADER = "file_header"
local ROW_KIND_COMMENT = "comment"
local KEY_FOCUS_REVIEW = "d"
local KEY_FILES = "o"
local KEY_COMMENTS = "i"
local KEY_HIDE = "q"
local KEY_OPEN = "<CR>"
local KEY_COMMENT = "c"
local KEY_DELETE_COMMENT = "D"
local KEY_TOGGLE_RESOLVED = "r"
local KEY_TOGGLE_COLLAPSED = "x"
local NAMESPACE = vim.api.nvim_create_namespace("peers-sidebar")
local HIGHLIGHT_DELTA_ADDED = "PeersSidebarDeltaAdded"
local HIGHLIGHT_DELTA_REMOVED = "PeersSidebarDeltaRemoved"
local HIGHLIGHT_DELTA_POSITIVE = "PeersSidebarDeltaPositive"
local HIGHLIGHT_DELTA_NEGATIVE = "PeersSidebarDeltaNegative"
local HIGHLIGHT_DELTA_NEUTRAL = "PeersSidebarDeltaNeutral"
local HIGHLIGHT_TAB_ACTIVE = "PeersSidebarTabActive"
local HIGHLIGHT_TAB_INACTIVE = "PeersSidebarTabInactive"
local HIGHLIGHT_TAB_FILL = "PeersSidebarTabFill"
local HIGHLIGHT_THREAD_META = "PeersSidebarThreadMeta"
local HIGHLIGHT_COMMENT_ELISION = "PeersSidebarCommentElision"
local HIGHLIGHT_THREAD_LOCATION_NOTE = "PeersSidebarThreadLocationNote"
local HIGHLIGHT_THREAD_RESOLVED = "PeersSidebarThreadResolved"
local STATUSLINE_ESCAPE_PATTERN = "([%%])"

M.MODE_FILES = MODE_FILES
M.MODE_COMMENTS = MODE_COMMENTS

local sidebar_review_by_buf = {}

local function highlight_fg(groups, fallback)
  for _, group in ipairs(groups) do
    local ok, highlight = pcall(vim.api.nvim_get_hl, 0, { name = group, link = false })
    if ok and highlight and highlight.fg then
      return highlight.fg
    end
  end
  return fallback
end

local function define_highlights()
  local add_fg = highlight_fg({ "GitSignsAdd", "Added", "DiagnosticOk" }, 0x3fb950)
  local delete_fg = highlight_fg({ "GitSignsDelete", "Removed", "DiagnosticError" }, 0xf85149)
  local change_fg = highlight_fg({ "GitSignsChange", "Changed", "DiagnosticWarn" }, 0xd29922)
  local info_fg = highlight_fg({ "DiagnosticInfo", "Identifier" }, 0x58a6ff)
  local muted_fg = highlight_fg({ "Comment", "LineNr" }, 0x8b949e)
  local warning_fg = highlight_fg({ "WarningMsg", "DiagnosticWarn" }, 0xd29922)
  pcall(vim.api.nvim_set_hl, 0, "PeersSidebarStatusAdded", { default = true, fg = add_fg, bold = true })
  pcall(vim.api.nvim_set_hl, 0, "PeersSidebarStatusDeleted", { default = true, fg = delete_fg, bold = true })
  pcall(vim.api.nvim_set_hl, 0, "PeersSidebarStatusModified", { default = true, fg = change_fg, bold = true })
  pcall(vim.api.nvim_set_hl, 0, "PeersSidebarStatusRenamed", { default = true, fg = info_fg, bold = true })
  pcall(vim.api.nvim_set_hl, 0, "PeersSidebarStatusUnchanged", { default = true, fg = muted_fg })
  pcall(vim.api.nvim_set_hl, 0, "PeersSidebarStatusBinary", { default = true, fg = warning_fg, bold = true })
  pcall(vim.api.nvim_set_hl, 0, HIGHLIGHT_DELTA_ADDED, { default = true, fg = add_fg })
  pcall(vim.api.nvim_set_hl, 0, HIGHLIGHT_DELTA_REMOVED, { default = true, fg = delete_fg })
  pcall(vim.api.nvim_set_hl, 0, HIGHLIGHT_DELTA_POSITIVE, { default = true, fg = add_fg })
  pcall(vim.api.nvim_set_hl, 0, HIGHLIGHT_DELTA_NEGATIVE, { default = true, fg = delete_fg })
  pcall(vim.api.nvim_set_hl, 0, HIGHLIGHT_DELTA_NEUTRAL, { default = true, fg = muted_fg })
  pcall(vim.api.nvim_set_hl, 0, HIGHLIGHT_TAB_ACTIVE, { default = true, link = "TabLineSel" })
  pcall(vim.api.nvim_set_hl, 0, HIGHLIGHT_TAB_INACTIVE, { default = true, link = "TabLine" })
  pcall(vim.api.nvim_set_hl, 0, HIGHLIGHT_TAB_FILL, { default = true, link = "TabLineFill" })
  pcall(vim.api.nvim_set_hl, 0, HIGHLIGHT_THREAD_META, { default = true, link = "PeersDiffThreadMeta" })
  pcall(vim.api.nvim_set_hl, 0, HIGHLIGHT_COMMENT_ELISION, { default = true, link = "Comment" })
  pcall(vim.api.nvim_set_hl, 0, HIGHLIGHT_THREAD_LOCATION_NOTE, { default = true, link = "PeersDiffThreadLocationNote" })
  pcall(vim.api.nvim_set_hl, 0, HIGHLIGHT_THREAD_RESOLVED, { default = true, fg = add_fg, bold = true })
end

local function existing_buffer(name)
  for _, buf in ipairs(vim.api.nvim_list_bufs()) do
    if vim.api.nvim_buf_is_valid(buf) and vim.api.nvim_buf_get_name(buf) == name then
      return buf
    end
  end
  return nil
end

local function set_lines(buf, lines)
  vim.bo[buf].readonly = false
  vim.bo[buf].modifiable = true
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
  vim.bo[buf].modified = false
  vim.bo[buf].modifiable = false
  vim.bo[buf].readonly = true
end

local function set_sidebar_options(buf)
  vim.bo[buf].buftype = "nofile"
  vim.bo[buf].bufhidden = "hide"
  vim.bo[buf].buflisted = false
  vim.bo[buf].swapfile = false
  vim.bo[buf].filetype = FILETYPE
  vim.bo[buf].modified = false
end

local function set_window_options(win)
  vim.wo[win].number = false
  vim.wo[win].relativenumber = false
  vim.wo[win].signcolumn = "no"
  vim.wo[win].foldcolumn = "0"
  vim.wo[win].wrap = false
  vim.wo[win].cursorline = true
  vim.wo[win].winfixbuf = true
  vim.wo[win].winfixwidth = true
  pcall(function()
    vim.wo[win].winbar = " "
  end)
end

local function review_buf_for(buf, states)
  if states[buf] then
    return buf
  end
  return sidebar_review_by_buf[buf]
end

local function review_state_for(buf, states)
  local review_buf = review_buf_for(buf or vim.api.nvim_get_current_buf(), states)
  if not review_buf then
    return nil, nil
  end
  return review_buf, states[review_buf]
end

local function review_window(review_buf)
  local current = vim.api.nvim_get_current_win()
  if vim.api.nvim_win_is_valid(current) and vim.api.nvim_win_get_buf(current) == review_buf then
    return current
  end
  local wins = vim.fn.win_findbuf(review_buf)
  return wins[1]
end

local function window_valid(state)
  return state and state.sidebar_win and vim.api.nvim_win_is_valid(state.sidebar_win)
end

local function composer_active(state)
  return state
    and (
      (state.composer_win and vim.api.nvim_win_is_valid(state.composer_win))
      or (state.composer_buf and vim.api.nvim_buf_is_valid(state.composer_buf))
    )
end

local function sidebar_windows(state)
  if not state or not state.sidebar_buf or not vim.api.nvim_buf_is_valid(state.sidebar_buf) then
    return {}
  end
  return vim.fn.win_findbuf(state.sidebar_buf)
end

local function close_sidebar_windows(state, keep)
  for _, win in ipairs(sidebar_windows(state)) do
    if win ~= keep and vim.api.nvim_win_is_valid(win) then
      if #vim.api.nvim_list_wins() <= 1 then
        local replacement = vim.api.nvim_create_buf(true, false)
        vim.wo[win].winfixbuf = false
        pcall(vim.api.nvim_win_set_buf, win, replacement)
      else
        pcall(vim.api.nvim_win_close, win, true)
      end
    end
  end
end

local function adopt_existing_window(state)
  local wins = sidebar_windows(state)
  local win = wins[1]
  if not win or not vim.api.nvim_win_is_valid(win) then
    return nil
  end
  state.sidebar_win = win
  set_window_options(win)
  close_sidebar_windows(state, win)
  return win
end

local function name_for_review(review_buf)
  return BUFFER_PREFIX .. vim.fn.fnamemodify(vim.api.nvim_buf_get_name(review_buf), ":t")
end

local function ensure_buffer(review_buf, state)
  define_highlights()
  if state.sidebar_buf and vim.api.nvim_buf_is_valid(state.sidebar_buf) then
    return state.sidebar_buf
  end

  local name = name_for_review(review_buf)
  local buf = existing_buffer(name) or vim.api.nvim_create_buf(false, true)
  vim.api.nvim_buf_set_name(buf, name)
  set_sidebar_options(buf)
  sidebar_review_by_buf[buf] = review_buf
  state.sidebar_buf = buf
  if not state.sidebar_augroup then
    state.sidebar_augroup = vim.api.nvim_create_augroup("peers-sidebar-" .. buf, { clear = true })
    vim.api.nvim_create_autocmd({ "BufEnter", "WinEnter" }, {
      group = state.sidebar_augroup,
      buffer = buf,
      callback = function()
        if composer_active(state) then
          return
        end
        state.sidebar_has_focus = true
      end,
    })
    vim.api.nvim_create_autocmd("WinLeave", {
      group = state.sidebar_augroup,
      buffer = buf,
      callback = function()
        state.sidebar_window_closing = true
        vim.schedule(function()
          state.sidebar_window_closing = false
        end)
      end,
    })
    vim.api.nvim_create_autocmd("BufWinLeave", {
      group = state.sidebar_augroup,
      buffer = buf,
      callback = function()
        if not state.sidebar_internal_close then
          state.sidebar_requested = false
        end
        state.sidebar_has_focus = false
        state.sidebar_win = nil
        state.sidebar_window_closing = true
        vim.schedule(function()
          state.sidebar_window_closing = false
        end)
      end,
    })
    vim.api.nvim_create_autocmd("BufWipeout", {
      group = state.sidebar_augroup,
      buffer = buf,
      callback = function()
        sidebar_review_by_buf[buf] = nil
      end,
    })
  end
  return buf
end

local function review_width(review_buf)
  local win = review_window(review_buf)
  if not win or not vim.api.nvim_win_is_valid(win) then
    return 0
  end
  return vim.api.nvim_win_get_width(win)
end

local function can_show(review_buf, state)
  local width = review_width(review_buf)
  if window_valid(state) then
    return width > MIN_REVIEW_WIDTH
  end
  return width - WIDTH > MIN_REVIEW_WIDTH
end

local function current_window_kind(review_buf, state)
  local current = vim.api.nvim_get_current_win()
  local current_buf = vim.api.nvim_win_get_buf(current)
  if composer_active(state) then
    if state.composer_buf and current_buf == state.composer_buf then
      return "composer"
    end
    if state.composer_return_sidebar_focus and state.sidebar_buf and current_buf == state.sidebar_buf then
      return "composer"
    end
  end
  if current_buf == review_buf then
    return "review"
  end
  if state and state.sidebar_buf and current_buf == state.sidebar_buf then
    return "sidebar"
  end
  return "other"
end

local function should_show(review_buf, state, focus, window_kind)
  if focus == true then
    return true
  end
  if not state.sidebar_requested then
    return false
  end
  if window_kind == "sidebar" then
    return true
  end
  if window_kind == "composer" then
    return window_valid(state) or can_show(review_buf, state)
  end
  if state.sidebar_has_focus then
    return true
  end
  if window_kind == "review" then
    return can_show(review_buf, state)
  end
  return window_valid(state) or can_show(review_buf, state)
end

local function ensure_window(review_buf, state, focus)
  ensure_buffer(review_buf, state)

  if window_valid(state) then
    set_window_options(state.sidebar_win)
    close_sidebar_windows(state, state.sidebar_win)
    return state.sidebar_win
  end

  local existing = adopt_existing_window(state)
  if existing then
    return existing
  end

  if state.sidebar_creating then
    return nil
  end

  local review_win = review_window(review_buf)
  if not review_win or not vim.api.nvim_win_is_valid(review_win) then
    return nil
  end

  local current = vim.api.nvim_get_current_win()
  local sidebar_buf = state.sidebar_buf
  state.sidebar_creating = true
  vim.api.nvim_set_current_win(review_win)
  vim.cmd("botright vertical " .. WIDTH .. "split")
  local sidebar_win = vim.api.nvim_get_current_win()
  vim.api.nvim_win_set_buf(sidebar_win, sidebar_buf)
  vim.api.nvim_win_set_width(sidebar_win, WIDTH)
  set_window_options(sidebar_win)
  state.sidebar_win = sidebar_win
  state.sidebar_creating = false
  close_sidebar_windows(state, sidebar_win)
  if not focus and current and vim.api.nvim_win_is_valid(current) then
    vim.api.nvim_set_current_win(current)
    state.sidebar_has_focus = false
    state.sidebar_window_closing = false
  end
  return sidebar_win
end

local function sidebar_counts(state)
  if state.sidebar_counts then
    return {
      files = state.sidebar_counts.files or 0,
      comments = state.sidebar_counts.comments or 0,
    }
  end

  local files = {}
  local threads = {}
  local file_count = 0
  local comment_count = 0
  for _, row in ipairs(state.rows or {}) do
    if row.kind == ROW_KIND_FILE_HEADER and row.path and not files[row.path] then
      files[row.path] = true
      file_count = file_count + 1
    elseif row.kind == ROW_KIND_COMMENT and row.thread_id and not threads[row.thread_id] then
      threads[row.thread_id] = true
      comment_count = comment_count + 1
    end
  end
  return {
    files = file_count,
    comments = comment_count,
  }
end

local function tab_part(mode, key, label, count, active_mode)
  local highlight = mode == active_mode and HIGHLIGHT_TAB_ACTIVE or HIGHLIGHT_TAB_INACTIVE
  return {
    text = string.format(" [%s] %s %d ", key, label, count),
    highlight = highlight,
  }
end

local function statusline_escape(text)
  return text:gsub(STATUSLINE_ESCAPE_PATTERN, "%%%1")
end

local function sidebar_winbar(state)
  local counts = sidebar_counts(state)
  local active_mode = state.sidebar_mode or MODE_FILES
  local tabs = {
    tab_part(MODE_COMMENTS, KEY_COMMENTS, "Comments", counts.comments, active_mode),
    tab_part(MODE_FILES, KEY_FILES, "Files", counts.files, active_mode),
  }
  local parts = {}
  for _, tab in ipairs(tabs) do
    table.insert(parts, "%#" .. tab.highlight .. "#" .. statusline_escape(tab.text))
  end
  table.insert(parts, "%#" .. HIGHLIGHT_TAB_FILL .. "#")
  return table.concat(parts, "")
end

local function clamp_cursor(buf, line, col)
  local line_count = math.max(1, vim.api.nvim_buf_line_count(buf))
  line = math.max(1, math.min(line or 1, line_count))
  local text = vim.api.nvim_buf_get_lines(buf, line - 1, line, false)[1] or ""
  col = math.max(0, math.min(col or 0, #text))
  return line, col
end

local function remember_cursor(state)
  if not window_valid(state) then
    return
  end
  state.sidebar_cursor_by_mode = state.sidebar_cursor_by_mode or {}
  state.sidebar_cursor_by_mode[state.sidebar_mode or MODE_FILES] = vim.api.nvim_win_get_cursor(state.sidebar_win)
end

local function restore_cursor(state)
  if not window_valid(state) then
    return
  end
  local cursor = (state.sidebar_cursor_by_mode or {})[state.sidebar_mode or MODE_FILES] or { 1, 0 }
  local line, col = clamp_cursor(state.sidebar_buf, cursor[1], cursor[2])
  pcall(vim.api.nvim_win_set_cursor, state.sidebar_win, { line, col })
end

local function render(review_buf, state)
  remember_cursor(state)
  local sidebar_buf = ensure_buffer(review_buf, state)
  local panel
  if state.sidebar_mode == MODE_COMMENTS then
    panel = state.sidebar and state.sidebar.comments or nil
  else
    panel = state.sidebar and state.sidebar.files or nil
  end
  local lines = panel and panel.lines or {}
  local rows = panel and panel.rows or {}
  local highlights = panel and panel.highlights or {}
  set_lines(sidebar_buf, lines)
  state.sidebar_rows = rows
  if window_valid(state) then
    pcall(function()
      vim.wo[state.sidebar_win].winbar = sidebar_winbar(state)
    end)
  end
  vim.api.nvim_buf_clear_namespace(sidebar_buf, NAMESPACE, 0, -1)
  for _, highlight in ipairs(highlights) do
    vim.api.nvim_buf_set_extmark(sidebar_buf, NAMESPACE, highlight.line, highlight.start_col, {
      end_col = highlight.end_col,
      hl_group = highlight.group,
    })
  end
  restore_cursor(state)
end

function M.close(state)
  state.sidebar_internal_close = true
  state.sidebar_window_closing = true
  close_sidebar_windows(state, nil)
  state.sidebar_internal_close = false
  state.sidebar_win = nil
  state.sidebar_creating = false
  vim.schedule(function()
    state.sidebar_window_closing = false
  end)
end

function M.detach(state)
  M.close(state)
  local sidebar_buf = state and state.sidebar_buf or nil
  if sidebar_buf then
    sidebar_review_by_buf[state.sidebar_buf] = nil
  end
  if state and state.sidebar_augroup then
    pcall(vim.api.nvim_del_augroup_by_id, state.sidebar_augroup)
    state.sidebar_augroup = nil
  end
  if sidebar_buf and vim.api.nvim_buf_is_valid(sidebar_buf) then
    pcall(vim.api.nvim_buf_delete, sidebar_buf, { force = true })
  end
  if state then
    state.sidebar_buf = nil
    state.sidebar_win = nil
    state.sidebar_rows = nil
  end
end

local function set_sidebar_keymaps(buf, states)
  vim.keymap.set("n", KEY_OPEN, function()
    M.open_item(buf, states)
  end, { buffer = buf, desc = "Open Peers sidebar item", nowait = true })
  vim.keymap.set("n", KEY_COMMENT, function()
    M.review_action(buf, states, "comment")
  end, { buffer = buf, desc = "Comment or reply in Peers review", nowait = true })
  vim.keymap.set("n", "A", function()
    M.review_action(buf, states, "agent_review_open")
  end, { buffer = buf, desc = "Ask agent to review all open Peers threads", nowait = true })
  vim.keymap.set("n", "R", function()
    M.review_action(buf, states, "agent_complete")
  end, { buffer = buf, desc = "Ask agent to respond and resolve Peers thread", nowait = true })
  vim.keymap.set("n", "C", function()
    M.review_action(buf, states, "agent_comment")
  end, { buffer = buf, desc = "Ask agent to comment on Peers thread", nowait = true })
  vim.keymap.set("n", "X", function()
    M.review_action(buf, states, "agent_commit")
  end, { buffer = buf, desc = "Ask agent to commit current changes", nowait = true })
  vim.keymap.set("n", KEY_DELETE_COMMENT, function()
    M.review_action(buf, states, "delete")
  end, { buffer = buf, desc = "Delete Peers comment", nowait = true })
  vim.keymap.set("n", KEY_TOGGLE_RESOLVED, function()
    M.review_action(buf, states, "toggle_resolved")
  end, { buffer = buf, desc = "Resolve or reopen Peers thread", nowait = true })
  vim.keymap.set("n", KEY_TOGGLE_COLLAPSED, function()
    M.review_action(buf, states, "toggle_collapsed")
  end, { buffer = buf, desc = "Collapse or expand Peers thread", nowait = true })
  vim.keymap.set("n", KEY_HIDE, function()
    M.hide(buf, states)
  end, { buffer = buf, desc = "Hide Peers sidebar", nowait = true })
  M.set_review_keymaps(buf, states)
end

function M.update(review_buf, states, focus, event)
  local current = vim.api.nvim_get_current_win()
  local state = states[review_buf]
  local window_kind = current_window_kind(review_buf, state)
  if state and state.sidebar_window_closing and event ~= "WinResized" and focus ~= true then
    return
  end
  if not state or not state.sidebar_requested then
    if state then
      M.close(state)
    end
    return
  end

  if not review_window(review_buf) then
    state.sidebar_has_focus = false
    M.close(state)
    return
  end

  local restore_sidebar_focus = event == "WinResized" and state.sidebar_has_focus
  if restore_sidebar_focus then
    window_kind = "sidebar"
  end

  if window_kind == "sidebar" then
    state.sidebar_has_focus = true
  end

  if not should_show(review_buf, state, focus, window_kind) then
    M.close(state)
    return
  end

  local win = ensure_window(review_buf, state, focus)
  if not win then
    return
  end
  set_sidebar_keymaps(state.sidebar_buf, states)
  render(review_buf, state)
  if (focus or restore_sidebar_focus) and vim.api.nvim_win_is_valid(win) then
    vim.api.nvim_set_current_win(win)
  elseif vim.api.nvim_win_is_valid(current) then
    vim.api.nvim_set_current_win(current)
  end
end

function M.update_preserving_focus(review_buf, states, event)
  local current = vim.api.nvim_get_current_win()
  local state = states[review_buf]
  local restore_sidebar_focus = event == "WinResized" and state and state.sidebar_has_focus
  M.update(review_buf, states, false, event)
  if restore_sidebar_focus and state and window_valid(state) then
    vim.api.nvim_set_current_win(state.sidebar_win)
    return
  end
  if current and vim.api.nvim_win_is_valid(current) then
    vim.api.nvim_set_current_win(current)
  end
end

function M.focus_review(buf, states)
  local review_buf = review_buf_for(buf or vim.api.nvim_get_current_buf(), states)
  if not review_buf then
    return
  end
  local state = states[review_buf]
  if state then
    state.sidebar_has_focus = false
  end
  if state and not can_show(review_buf, state) then
    M.close(state)
  end
  local win = review_window(review_buf)
  if win and vim.api.nvim_win_is_valid(win) then
    vim.api.nvim_set_current_win(win)
  end
end

function M.show(buf, states, mode, focus)
  local review_buf, state = review_state_for(buf, states)
  if not state then
    return
  end
  state.sidebar_requested = true
  state.sidebar_mode = mode or state.sidebar_mode or MODE_FILES
  if focus ~= false then
    state.sidebar_has_focus = true
  end
  M.update(review_buf, states, focus ~= false)
end

function M.hide(buf, states)
  local _, state = review_state_for(buf, states)
  if not state then
    return
  end
  state.sidebar_requested = false
  state.sidebar_has_focus = false
  M.close(state)
end

function M.open_item(buf, states)
  local review_buf, state = review_state_for(buf, states)
  if not state or not state.sidebar_rows then
    return
  end
  local cursor = vim.api.nvim_win_get_cursor(0)
  local item = state.sidebar_rows[cursor[1]]
  if not item or not item.target_line then
    return
  end
  local win = review_window(review_buf)
  if not win or not vim.api.nvim_win_is_valid(win) then
    return
  end
  state.sidebar_has_focus = false
  local line, col = clamp_cursor(review_buf, item.target_line, 0)
  vim.api.nvim_set_current_win(win)
  vim.api.nvim_win_set_cursor(win, { line, col })
  pcall(function()
    require("peers.buffer").remember_current_view(review_buf)
  end)
  if window_valid(state) then
    state.sidebar_has_focus = true
    vim.api.nvim_set_current_win(state.sidebar_win)
  end
end

function M.review_action(buf, states, action)
  local review_buf, state = review_state_for(buf, states)
  if not state then
    return
  end
  local buffer = require("peers.buffer")
  if action == "agent_review_open" then
    buffer.agent_review_open_threads(review_buf)
    return
  elseif action == "agent_commit" then
    buffer.agent_commit_changes(review_buf)
    return
  end
  if not state.sidebar_rows then
    return
  end
  local cursor = vim.api.nvim_win_get_cursor(0)
  local item = state.sidebar_rows[cursor[1]]
  if not item or not item.target_line then
    return
  end
  local row = state.rows and state.rows[item.target_line] or nil
  if action == "comment" then
    buffer.comment_or_reply(review_buf, row, item.target_line, {
      return_win = vim.api.nvim_get_current_win(),
    })
  elseif action == "agent_complete" then
    buffer.agent_complete_selected_thread(review_buf, row)
  elseif action == "agent_comment" then
    buffer.agent_comment_selected_thread(review_buf, row)
  elseif action == "delete" then
    buffer.delete_selected_comment(review_buf, row)
  elseif action == "toggle_resolved" then
    buffer.toggle_selected_thread_resolved(review_buf, row)
  elseif action == "toggle_collapsed" then
    buffer.toggle_selected_thread_collapsed(review_buf, row)
  end
end

function M.mark_review_active(review_buf, states)
  local state = states[review_buf]
  if state and state.sidebar_has_focus and window_valid(state) then
    return
  end
  if state then
    state.sidebar_has_focus = false
  end
  if state and not can_show(review_buf, state) then
    M.close(state)
  end
end

function M.set_review_keymaps(buf, states)
  vim.keymap.set("n", KEY_FOCUS_REVIEW, function()
    M.focus_review(buf, states)
  end, { buffer = buf, desc = "Focus Peers diff", nowait = true })
  for _, key in ipairs({ KEY_FILES, string.upper(KEY_FILES) }) do
    vim.keymap.set("n", key, function()
      M.show(buf, states, MODE_FILES, true)
    end, { buffer = buf, desc = "Show Peers files sidebar", nowait = true })
  end
  for _, key in ipairs({ KEY_COMMENTS, string.upper(KEY_COMMENTS) }) do
    vim.keymap.set("n", key, function()
      M.show(buf, states, MODE_COMMENTS, true)
    end, { buffer = buf, desc = "Show Peers comments sidebar", nowait = true })
  end
end

return M
