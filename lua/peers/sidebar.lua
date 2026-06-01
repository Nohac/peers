local M = {}

local BUFFER_PREFIX = "peers://review-sidebar/"
local FILETYPE = "peerssidebar"
local MODE_FILES = "files"
local MODE_COMMENTS = "comments"
local TITLE_FILES = "Files"
local TITLE_COMMENTS = "Comments"
local EMPTY_FILES = "No files"
local EMPTY_COMMENTS = "No comments"
local WIDTH = 36
local MIN_REVIEW_WIDTH = 120
local ROW_KIND_FILE_HEADER = "file_header"
local ROW_KIND_COMMENT = "comment"
local KEY_FOCUS_REVIEW = "d"
local KEY_FILES = "o"
local KEY_COMMENTS = "i"
local KEY_HIDE = "q"
local KEY_OPEN = "<CR>"

M.MODE_FILES = MODE_FILES
M.MODE_COMMENTS = MODE_COMMENTS

local sidebar_review_by_buf = {}

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

local function sidebar_windows(state)
  if not state or not state.sidebar_buf or not vim.api.nvim_buf_is_valid(state.sidebar_buf) then
    return {}
  end
  return vim.fn.win_findbuf(state.sidebar_buf)
end

local function close_sidebar_windows(state, keep)
  for _, win in ipairs(sidebar_windows(state)) do
    if win ~= keep and vim.api.nvim_win_is_valid(win) then
      pcall(vim.api.nvim_win_close, win, true)
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
        state.sidebar_has_focus = true
      end,
    })
    vim.api.nvim_create_autocmd("WinLeave", {
      group = state.sidebar_augroup,
      buffer = buf,
      callback = function()
        state.sidebar_has_focus = false
        state.sidebar_recent_focus = true
        state.sidebar_window_closing = true
        vim.schedule(function()
          state.sidebar_recent_focus = false
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
  return review_width(review_buf) > MIN_REVIEW_WIDTH
end

local function current_window_kind(review_buf, state)
  local current = vim.api.nvim_get_current_win()
  local current_buf = vim.api.nvim_win_get_buf(current)
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
  end
  return sidebar_win
end

local function shorten(text, width)
  text = text or ""
  if #text <= width then
    return text
  end
  if width <= 3 then
    return text:sub(-width)
  end
  return "..." .. text:sub(-(width - 3))
end

local function push_line(lines, rows, text, target)
  table.insert(lines, text)
  table.insert(rows, target or {})
end

local function comment_counts(rows)
  local counts = {}
  local seen = {}
  for _, row in ipairs(rows or {}) do
    if row.kind == ROW_KIND_COMMENT and row.path and row.thread_id and not seen[row.thread_id] then
      seen[row.thread_id] = true
      counts[row.path] = (counts[row.path] or 0) + 1
    end
  end
  return counts
end

local function build_files(state)
  local lines = {}
  local rows = {}
  local seen = {}
  local counts = comment_counts(state.rows)
  push_line(lines, rows, TITLE_FILES, { title = true })
  push_line(lines, rows, string.rep("-", WIDTH - 1), { title = true })

  for index, row in ipairs(state.rows or {}) do
    if row.kind == ROW_KIND_FILE_HEADER and row.path and not seen[row.path] then
      seen[row.path] = true
      local count = counts[row.path] or 0
      local prefix = count > 0 and string.format("%2d ", count) or "   "
      push_line(lines, rows, prefix .. shorten(row.path, WIDTH - #prefix - 1), {
        target_line = index,
        path = row.path,
      })
    end
  end

  if #lines == 2 then
    push_line(lines, rows, EMPTY_FILES, {})
  end
  return lines, rows
end

local function thread_label(row)
  local path = row.path or "review"
  if row.source_line then
    return path .. ":" .. row.source_line
  end
  return path
end

local function build_comments(state)
  local threads = {}
  local by_id = {}
  for index, row in ipairs(state.rows or {}) do
    if row.kind == ROW_KIND_COMMENT and row.thread_id then
      local thread = by_id[row.thread_id]
      if not thread then
        thread = {
          target_line = index,
          label = thread_label(row),
          resolved = row.resolved == true,
        }
        by_id[row.thread_id] = thread
        table.insert(threads, thread)
      end
      if row.comment_body and not thread.body then
        thread.body = row.comment_body
      end
    end
  end

  local lines = {}
  local rows = {}
  push_line(lines, rows, TITLE_COMMENTS, { title = true })
  push_line(lines, rows, string.rep("-", WIDTH - 1), { title = true })

  for _, thread in ipairs(threads) do
    local status = thread.resolved and "x " or "* "
    push_line(lines, rows, status .. shorten(thread.label, WIDTH - 3), {
      target_line = thread.target_line,
    })
    if thread.body then
      push_line(lines, rows, "  " .. shorten(thread.body, WIDTH - 3), {
        target_line = thread.target_line,
      })
    end
  end

  if #lines == 2 then
    push_line(lines, rows, EMPTY_COMMENTS, {})
  end
  return lines, rows
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
  local lines, rows
  if state.sidebar_mode == MODE_COMMENTS then
    lines, rows = build_comments(state)
  else
    lines, rows = build_files(state)
  end
  set_lines(sidebar_buf, lines)
  state.sidebar_rows = rows
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
  if state and state.sidebar_buf then
    sidebar_review_by_buf[state.sidebar_buf] = nil
  end
  if state and state.sidebar_augroup then
    pcall(vim.api.nvim_del_augroup_by_id, state.sidebar_augroup)
    state.sidebar_augroup = nil
  end
end

local function set_sidebar_keymaps(buf, states)
  vim.keymap.set("n", KEY_OPEN, function()
    M.open_item(buf, states)
  end, { buffer = buf, desc = "Open Peers sidebar item", nowait = true })
  vim.keymap.set("n", KEY_HIDE, function()
    M.hide(buf, states)
  end, { buffer = buf, desc = "Hide Peers sidebar", nowait = true })
  M.set_review_keymaps(buf, states)
end

function M.update(review_buf, states, focus, event)
  local current = vim.api.nvim_get_current_win()
  local state = states[review_buf]
  local window_kind = current_window_kind(review_buf, state)
  if state and state.sidebar_window_closing and event ~= "WinResized" then
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

  local preserve_sidebar_focus = event == "WinResized" and (state.sidebar_has_focus == true or state.sidebar_recent_focus == true)
  if preserve_sidebar_focus then
    window_kind = "sidebar"
  elseif window_kind == "sidebar" then
    state.sidebar_has_focus = true
  elseif window_kind == "review" and not state.sidebar_has_focus then
    state.sidebar_has_focus = false
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
  if (focus or preserve_sidebar_focus) and vim.api.nvim_win_is_valid(win) then
    vim.api.nvim_set_current_win(win)
    if preserve_sidebar_focus then
      vim.schedule(function()
        if vim.api.nvim_win_is_valid(win) then
          pcall(vim.api.nvim_set_current_win, win)
        end
      end)
    end
  elseif vim.api.nvim_win_is_valid(current) then
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
  for _, key in ipairs({ KEY_FOCUS_REVIEW, string.upper(KEY_FOCUS_REVIEW) }) do
    vim.keymap.set("n", key, function()
      M.focus_review(buf, states)
    end, { buffer = buf, desc = "Focus Peers diff", nowait = true })
  end
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
