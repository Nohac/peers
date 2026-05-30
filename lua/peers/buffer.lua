local lsp = require("peers.lsp")

local M = {}

local BUFFER_PREFIX = "peers://review/"
local FILETYPE = "peersdiff"
local NAMESPACE = vim.api.nvim_create_namespace("peers-review")
local SOURCE_NAMESPACE = vim.api.nvim_create_namespace("peers-review-source")
local DIAGNOSTIC_NAMESPACE = vim.api.nvim_create_namespace("peers-review-diagnostics")
local ADD_FALLBACK_FG = "#3fb950"
local DELETE_FALLBACK_FG = "#f85149"
local THREAD_FALLBACK_FG = "#58a6ff"
local NORMAL_FALLBACK_FG = "#f0f6fc"
local NORMAL_FALLBACK_BG = "#000000"
local HIGHLIGHT_FG = "fg"
local HIGHLIGHT_BG = "bg"
local HIGHLIGHT_BOLD = "bold"
local HIGHLIGHT_DIRTY_TITLE = "PeersDiffDirtyTitle"
local HIGHLIGHT_DIRTY_TEXT = "PeersDiffDirtyText"
local GUTTER_BACKGROUND_BLEND = 0.42
local LINE_BACKGROUND_BLEND = 0.16
local GUTTER_FOREGROUND_GROUPS = {
  "Normal",
}
local ADD_FOREGROUND_GROUPS = {
  "GitSignsAdd",
  "Added",
  "DiagnosticOk",
  "DiffAdd",
}
local DELETE_FOREGROUND_GROUPS = {
  "GitSignsDelete",
  "Removed",
  "DiagnosticError",
  "DiffDelete",
}
local ADD_BACKGROUND_GROUPS = {
  "DiffAdd",
}
local DELETE_BACKGROUND_GROUPS = {
  "DiffDelete",
}
local THREAD_FOREGROUND_GROUPS = {
  "DiagnosticInfo",
  "Identifier",
}
local HIGHLIGHT_GROUPS = {
  PeersDiffFileHeader = { link = "Title" },
  PeersDiffHunkHeader = { link = "DiffChange" },
  PeersDiffLineNumber = { link = "LineNr" },
  PeersDiffEmptyTitle = { link = "Title" },
  PeersDiffEmptyText = { link = "Normal" },
  [HIGHLIGHT_DIRTY_TITLE] = { link = "DiagnosticError" },
  [HIGHLIGHT_DIRTY_TEXT] = { link = "WarningMsg" },
}
local ROW_SIDE_NEW = "new"
local ROW_SCOPE_LINE = "line"
local ROW_SCOPE_FILE = "file"
local ROW_KIND_FILE_HEADER = "file_header"
local ROW_KIND_DIRTY = "dirty"
local ROW_KIND_ADD = "add"
local ROW_KIND_CONTEXT = "context"
local ROW_KIND_DELETE = "delete"
local ROW_KIND_COMMENT = "comment"
local DIRTY_FILE_TITLE = "Unsaved changes in this file"
local DIRTY_FILE_MESSAGE = "Peers is hiding this file diff because Neovim has a modified buffer for it."
local DIRTY_FILE_HINT = "Write or reload the file, then refresh the review."
local DIRTY_FILE_DIAGNOSTIC_SOURCE = "peers"
local DIRTY_FILE_DIAGNOSTIC_MESSAGE = "Peers review diff hidden because this file has unsaved Neovim changes"
local DIRTY_FILE_INDENT = "  "
local SOURCE_PROXY_UNAVAILABLE = "Peers source LSP proxy is only available on current-side added or context lines"
local COMPOSER_TITLE = " Peers comment "
local COMPOSER_FILETYPE = "markdown"
local COMPOSER_INITIAL_LINE = ""
local COMPOSER_SUBMIT_MAP = "<C-s>"
local COMPOSER_CANCEL_MAP = "q"
local COMPOSER_HEIGHT = 7
local COMPOSER_MIN_WIDTH = 40
local COMPOSER_MAX_WIDTH = 88
local COMPOSER_GUTTER_COL = 14
local COMMENT_EMPTY_MESSAGE = "Peers comment is empty"
local COMMENT_UNAVAILABLE_MESSAGE = "Peers comment is only available on diff lines for now"
local OPEN_SOURCE_UNAVAILABLE_MESSAGE = "Peers can only open source files from file, diff, or comment rows"
local OPEN_SOURCE_MISSING_MESSAGE = "Peers source file does not exist: "
local COMMENT_REPLY_UNAVAILABLE_MESSAGE = "Peers reply is only available on comment threads"
local COMMENT_EDIT_UNAVAILABLE_MESSAGE = "Peers edit is only available on editable comments"
local COMMENT_DELETE_UNAVAILABLE_MESSAGE = "Peers delete is only available on editable comments"
local COMMENT_THREAD_UNAVAILABLE_MESSAGE = "Peers thread action is only available on comment threads"
local OPEN_SOURCE_KEY = "<CR>"
local OPEN_SOURCE_DESC = "Open source file"
local COMMENT_CONFIRM_CHOICES = "&Proceed\n&Cancel"
local COMMENT_CONFIRM_DEFAULT = 2
local COMMENT_CONFIRM_DANGER = "WarningMsg"
local COMMENT_EDIT_CONFIRM_TITLE = "Edit comment?"
local COMMENT_EDIT_CONFIRM_MESSAGE = "Editing this comment will remove later replies and thread status changes from the visible review state."
local COMMENT_DELETE_CONFIRM_TITLE = "Delete comment?"
local COMMENT_DELETE_CONFIRM_MESSAGE = "Deleting this comment will remove later replies and thread status changes from the visible review state."
local MIRROR_DEBOUNCE_MS = 30
local AUTOCMD_GROUP_PREFIX = "peers-review-source-"
local AUTOCMD_EVENTS = {
  "BufEnter",
  "WinEnter",
  "WinResized",
  "WinScrolled",
}
local VIEW_SAVE_EVENTS = {
  "BufLeave",
  "WinLeave",
  "WinScrolled",
}
local CACHE_KEY_SEPARATOR = ":"
local UNKNOWN_FILE_TIME = -1
local UNKNOWN_FILE_SIZE = -1
local SOURCE_HELPER_BUFFER_VAR = "peers_source_helper"
local SOURCE_HELPER_SIGNATURE_VAR = "peers_source_signature"
local RENDER_STATES = {}

local function existing_buffer(name)
  for _, buf in ipairs(vim.api.nvim_list_bufs()) do
    if vim.api.nvim_buf_is_valid(buf) and vim.api.nvim_buf_get_name(buf) == name then
      return buf
    end
  end
  return nil
end

local function set_review_options(buf)
  vim.bo[buf].buftype = "nofile"
  vim.bo[buf].bufhidden = "hide"
  vim.bo[buf].buflisted = true
  vim.bo[buf].swapfile = false
  vim.bo[buf].filetype = FILETYPE
end

local function define_highlights()
  for group, spec in pairs(HIGHLIGHT_GROUPS) do
    pcall(vim.api.nvim_set_hl, 0, group, vim.tbl_extend("force", { default = true }, spec))
  end
end

local function highlight_color(groups, key, fallback)
  for _, group in ipairs(groups) do
    local ok, highlight = pcall(vim.api.nvim_get_hl, 0, { name = group, link = false })
    if ok and highlight and highlight[key] then
      return highlight[key]
    end
  end
  return fallback
end

local function foreground_from(groups, fallback)
  return highlight_color(groups, HIGHLIGHT_FG, fallback)
end

local function background_from(groups, fallback)
  return highlight_color(groups, HIGHLIGHT_BG, fallback)
end

local function rgb_components(color)
  if type(color) == "string" then
    local normalized = color:gsub("^#", "")
    return tonumber(normalized:sub(1, 2), 16), tonumber(normalized:sub(3, 4), 16), tonumber(normalized:sub(5, 6), 16)
  end

  return math.floor(color / 65536) % 256, math.floor(color / 256) % 256, color % 256
end

local function rgb_color(red, green, blue)
  return string.format("#%02x%02x%02x", red, green, blue)
end

local function blend_color(color, base, amount)
  local red, green, blue = rgb_components(color)
  local base_red, base_green, base_blue = rgb_components(base)
  return rgb_color(
    math.floor(red * amount + base_red * (1 - amount)),
    math.floor(green * amount + base_green * (1 - amount)),
    math.floor(blue * amount + base_blue * (1 - amount))
  )
end

local function define_diff_gutter_highlights()
  local normal_fg = foreground_from(GUTTER_FOREGROUND_GROUPS, NORMAL_FALLBACK_FG)
  local normal_bg = background_from(GUTTER_FOREGROUND_GROUPS, NORMAL_FALLBACK_BG)
  local add_fg = foreground_from(ADD_FOREGROUND_GROUPS, ADD_FALLBACK_FG)
  local delete_fg = foreground_from(DELETE_FOREGROUND_GROUPS, DELETE_FALLBACK_FG)
  local add_gutter_bg = blend_color(add_fg, normal_bg, GUTTER_BACKGROUND_BLEND)
  local delete_gutter_bg = blend_color(delete_fg, normal_bg, GUTTER_BACKGROUND_BLEND)
  local add_line_bg = background_from(ADD_BACKGROUND_GROUPS, blend_color(add_fg, normal_bg, LINE_BACKGROUND_BLEND))
  local delete_line_bg = background_from(DELETE_BACKGROUND_GROUPS, blend_color(delete_fg, normal_bg, LINE_BACKGROUND_BLEND))
  pcall(vim.api.nvim_set_hl, 0, "PeersDiffAddGutterBackground", {
    default = true,
    [HIGHLIGHT_BG] = add_gutter_bg,
  })
  pcall(vim.api.nvim_set_hl, 0, "PeersDiffDeleteGutterBackground", {
    default = true,
    [HIGHLIGHT_BG] = delete_gutter_bg,
  })
  pcall(vim.api.nvim_set_hl, 0, "PeersDiffAddGutter", {
    default = true,
    [HIGHLIGHT_FG] = normal_fg,
    [HIGHLIGHT_BG] = add_gutter_bg,
    [HIGHLIGHT_BOLD] = true,
  })
  pcall(vim.api.nvim_set_hl, 0, "PeersDiffDeleteGutter", {
    default = true,
    [HIGHLIGHT_FG] = normal_fg,
    [HIGHLIGHT_BG] = delete_gutter_bg,
    [HIGHLIGHT_BOLD] = true,
  })
  pcall(vim.api.nvim_set_hl, 0, "PeersDiffAddLineBackground", {
    default = true,
    [HIGHLIGHT_BG] = add_line_bg,
  })
  pcall(vim.api.nvim_set_hl, 0, "PeersDiffDeleteLineBackground", {
    default = true,
    [HIGHLIGHT_BG] = delete_line_bg,
  })
  pcall(vim.api.nvim_set_hl, 0, "PeersDiffComment", {
    default = true,
    [HIGHLIGHT_FG] = normal_fg,
    [HIGHLIGHT_BG] = blend_color(normal_fg, normal_bg, 0.08),
  })
  local thread_fg = foreground_from(THREAD_FOREGROUND_GROUPS, THREAD_FALLBACK_FG)
  pcall(vim.api.nvim_set_hl, 0, "PeersDiffThreadAttachment", {
    default = true,
  })
  pcall(vim.api.nvim_set_hl, 0, "PeersDiffThreadBody", {
    default = true,
    [HIGHLIGHT_FG] = normal_fg,
  })
  pcall(vim.api.nvim_set_hl, 0, "PeersDiffThreadBorder", {
    default = true,
    [HIGHLIGHT_FG] = thread_fg,
  })
  pcall(vim.api.nvim_set_hl, 0, "PeersDiffThreadHeader", {
    default = true,
    [HIGHLIGHT_FG] = normal_fg,
    [HIGHLIGHT_BOLD] = true,
  })
  pcall(vim.api.nvim_set_hl, 0, "PeersDiffThreadMeta", {
    default = true,
    [HIGHLIGHT_FG] = thread_fg,
  })
  pcall(vim.api.nvim_set_hl, 0, "PeersDiffThreadRail", {
    default = true,
    [HIGHLIGHT_FG] = thread_fg,
    [HIGHLIGHT_BG] = thread_fg,
  })
end

local function set_lines(buf, lines)
  vim.bo[buf].readonly = false
  vim.bo[buf].modifiable = true
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
  vim.bo[buf].modifiable = false
  vim.bo[buf].readonly = true
end

local function file_buffer_name(root, path)
  return vim.fn.fnamemodify(root .. "/" .. path, ":p")
end

local function modified_buffer_for_path(root, path)
  local name = file_buffer_name(root, path)
  for _, buf in ipairs(vim.api.nvim_list_bufs()) do
    local buffer_name = vim.api.nvim_buf_get_name(buf)
    if
      vim.api.nvim_buf_is_valid(buf)
      and buffer_name ~= ""
      and vim.fn.fnamemodify(buffer_name, ":p") == name
      and vim.bo[buf].modified
    then
      return buf
    end
  end
  return nil
end

local function dirty_paths_for_render(root, render)
  local dirty = {}
  for _, row in ipairs(render.rows or {}) do
    if row.path and not dirty[row.path] and modified_buffer_for_path(root, row.path) then
      dirty[row.path] = true
    end
  end
  return dirty
end

local function dirty_warning_rows(path)
  return {
    {
      line = DIRTY_FILE_INDENT .. DIRTY_FILE_TITLE,
      row = { kind = ROW_KIND_DIRTY, path = path },
      group = HIGHLIGHT_DIRTY_TITLE,
    },
    {
      line = DIRTY_FILE_INDENT .. DIRTY_FILE_MESSAGE,
      row = { kind = ROW_KIND_DIRTY, path = path },
      group = HIGHLIGHT_DIRTY_TEXT,
    },
    {
      line = DIRTY_FILE_INDENT .. DIRTY_FILE_HINT,
      row = { kind = ROW_KIND_DIRTY, path = path },
      group = HIGHLIGHT_DIRTY_TEXT,
    },
  }
end

local function highlights_by_line(highlights)
  local by_line = {}
  for _, highlight in ipairs(highlights or {}) do
    local line = highlight.line
    if line then
      by_line[line] = by_line[line] or {}
      table.insert(by_line[line], highlight)
    end
  end
  return by_line
end

local function push_render_line(target, source_line, line, row, by_line)
  local next_line = #target.lines
  table.insert(target.lines, line)
  table.insert(target.rows, row)
  for _, highlight in ipairs(by_line[source_line] or {}) do
    table.insert(target.highlights, vim.tbl_extend("force", highlight, {
      line = next_line,
    }))
  end
end

local function push_dirty_diagnostic(diagnostics, line, end_col)
  table.insert(diagnostics, {
    lnum = line,
    col = 0,
    end_lnum = line,
    end_col = end_col,
    severity = vim.diagnostic.severity.ERROR,
    source = DIRTY_FILE_DIAGNOSTIC_SOURCE,
    message = DIRTY_FILE_DIAGNOSTIC_MESSAGE,
  })
end

local function push_dirty_warning(target, path, diagnostics)
  local first_warning_line = #target.lines
  for _, warning in ipairs(dirty_warning_rows(path)) do
    local line = #target.lines
    table.insert(target.lines, warning.line)
    table.insert(target.rows, warning.row)
    table.insert(target.highlights, {
      line = line,
      start_col = 0,
      end_col = #warning.line,
      group = warning.group,
    })
  end
  push_dirty_diagnostic(diagnostics, first_warning_line, #(target.lines[first_warning_line + 1] or ""))
end

local function mask_dirty_file_diffs(root, render)
  local dirty = dirty_paths_for_render(root, render)
  if next(dirty) == nil then
    render.diagnostics = {}
    return render
  end

  local by_line = highlights_by_line(render.highlights)
  local diagnostics = {}
  local masked = {
    lines = {},
    rows = {},
    highlights = {},
    diagnostics = diagnostics,
  }
  local index = 1

  while index <= #(render.rows or {}) do
    local row = render.rows[index]
    local line = render.lines[index] or ""
    if row and row.kind == ROW_KIND_FILE_HEADER and row.path and dirty[row.path] then
      push_render_line(masked, index - 1, line, row, by_line)
      push_dirty_warning(masked, row.path, diagnostics)
      index = index + 1
      while index <= #(render.rows or {}) do
        local next_row = render.rows[index]
        if next_row and next_row.kind == ROW_KIND_FILE_HEADER then
          break
        end
        index = index + 1
      end
    else
      push_render_line(masked, index - 1, line, row or {}, by_line)
      index = index + 1
    end
  end

  return masked
end

local function apply_structural_highlights(buf, highlights)
  vim.api.nvim_buf_clear_namespace(buf, NAMESPACE, 0, -1)
  for _, highlight in ipairs(highlights or {}) do
    vim.api.nvim_buf_set_extmark(buf, NAMESPACE, highlight.line, highlight.start_col, {
      end_col = highlight.end_col,
      hl_group = highlight.group,
    })
  end
end

local function apply_diagnostics(buf, diagnostics)
  vim.diagnostic.set(DIAGNOSTIC_NAMESPACE, buf, diagnostics or {}, {})
end

local function source_file_signature(full_path)
  local stat = vim.uv.fs_stat(full_path)
  if not stat then
    return table.concat({ UNKNOWN_FILE_TIME, UNKNOWN_FILE_SIZE }, CACHE_KEY_SEPARATOR)
  end

  local modified = stat.mtime or {}
  return table.concat({
    modified.sec or UNKNOWN_FILE_TIME,
    modified.nsec or 0,
    stat.size or UNKNOWN_FILE_SIZE,
  }, CACHE_KEY_SEPARATOR)
end

local function reload_source_helper_if_stale(buf, signature)
  if not vim.b[buf][SOURCE_HELPER_BUFFER_VAR] then
    return
  end
  if vim.b[buf][SOURCE_HELPER_SIGNATURE_VAR] == signature then
    return
  end
  if vim.bo[buf].modified then
    return
  end

  pcall(vim.api.nvim_buf_call, buf, function()
    vim.cmd("silent! edit!")
  end)
end

local function source_buffer(root, path)
  local full_path = vim.fn.fnamemodify(root .. "/" .. path, ":p")
  if vim.fn.filereadable(full_path) ~= 1 then
    return nil
  end

  local signature = source_file_signature(full_path)
  local existing = existing_buffer(full_path)
  local buf = vim.fn.bufadd(full_path)
  vim.fn.bufload(buf)
  if not existing then
    vim.bo[buf].buflisted = false
    vim.b[buf][SOURCE_HELPER_BUFFER_VAR] = true
  end
  reload_source_helper_if_stale(buf, signature)
  vim.b[buf][SOURCE_HELPER_SIGNATURE_VAR] = signature

  if vim.bo[buf].filetype == "" then
    local filetype = vim.filetype.match({ filename = full_path })
    if filetype then
      vim.bo[buf].filetype = filetype
    end
  end

  return buf
end

local function ensure_highlighter(buf)
  if vim.bo[buf].filetype == "" then
    return false
  end

  local ok_start = pcall(vim.treesitter.start, buf, vim.bo[buf].filetype)
  if not ok_start then
    return false
  end

  local ok_parse = pcall(function()
    local parser = vim.treesitter.get_parser(buf, vim.bo[buf].filetype)
    parser:parse(true)
  end)
  return ok_parse
end

local function capture_group_at(buf, row, col)
  local ok, captures = pcall(vim.treesitter.get_captures_at_pos, buf, row, col)
  if not ok or not captures or #captures == 0 then
    return nil
  end

  local capture = captures[#captures]
  if not capture or not capture.capture then
    return nil
  end
  return "@" .. capture.capture
end

local function source_line_segments(source_buf, source_line)
  local source_row = source_line - 1
  local source_text = vim.api.nvim_buf_get_lines(source_buf, source_row, source_row + 1, false)[1]
  if not source_text or source_text == "" then
    return {}
  end

  local segments = {}
  local active_group = nil
  local active_start = nil
  local byte_len = #source_text

  for col = 0, byte_len do
    local group = col < byte_len and capture_group_at(source_buf, source_row, col) or nil
    if group ~= active_group then
      if active_group and active_start and active_start < col then
        table.insert(segments, {
          start_col = active_start,
          end_col = col,
          group = active_group,
        })
      end
      active_group = group
      active_start = col
    end
  end

  return segments
end

local function row_is_mirrorable(row)
  return row
    and row.side == ROW_SIDE_NEW
    and (row.kind == ROW_KIND_ADD or row.kind == ROW_KIND_CONTEXT)
    and row.path
    and row.source_line
end

local function row_is_proxyable(row)
  return row_is_mirrorable(row)
end

local function row_is_commentable(row)
  return row
    and row.path
    and row.side
    and row.source_line
    and (row.kind == ROW_KIND_ADD or row.kind == ROW_KIND_CONTEXT or row.kind == ROW_KIND_DELETE)
end

local function row_is_source_jumpable(row)
  return row
    and row.path
    and (
      row.kind == ROW_KIND_ADD
      or row.kind == ROW_KIND_CONTEXT
      or row.kind == ROW_KIND_DELETE
      or row.kind == ROW_KIND_COMMENT
      or row.kind == ROW_KIND_FILE_HEADER
      or row.kind == ROW_KIND_DIRTY
    )
end

local function row_jump_line(row)
  return math.max(1, row.source_line or 1)
end

local function source_for_proxy_row(state, row)
  local source = state.source_lsp_buffers[row.path]
  if source == nil then
    source = source_buffer(state.root, row.path)
    state.source_lsp_buffers[row.path] = source or false
  end

  if source == false then
    return nil
  end
  return source
end

local function source_for_row(state, row)
  local source = state.source_buffers[row.path]
  if source == nil then
    source = source_buffer(state.root, row.path)
    if source and not ensure_highlighter(source) then
      source = false
    end
    state.source_buffers[row.path] = source or false
  end

  if source == false then
    return nil
  end
  return source
end

local function cache_for_file(state, row)
  local source = source_for_row(state, row)
  if not source then
    return nil, nil
  end

  local full_path = file_buffer_name(state.root, row.path)
  local signature = source_file_signature(full_path)
  local file_cache = state.source_segments[row.path]
  if not file_cache or file_cache.signature ~= signature then
    file_cache = {
      signature = signature,
      lines = {},
    }
    state.source_segments[row.path] = file_cache
  end
  return source, file_cache.lines
end

local function segments_for_row(state, row)
  local source, lines = cache_for_file(state, row)
  if not source or not lines then
    return {}
  end

  local cached = lines[row.source_line]
  if cached then
    return cached
  end

  lines[row.source_line] = source_line_segments(source, row.source_line)
  return lines[row.source_line]
end

local function apply_line_segments(buf, review_row, code_start_col, segments)
  local line = vim.api.nvim_buf_get_lines(buf, review_row, review_row + 1, false)[1]
  if not line then
    return
  end

  local line_len = #line
  for _, segment in ipairs(segments) do
    local start_col = math.min(line_len, code_start_col + segment.start_col)
    local end_col = math.min(line_len, code_start_col + segment.end_col)
    if start_col < end_col then
      vim.api.nvim_buf_set_extmark(buf, SOURCE_NAMESPACE, review_row, start_col, {
        end_col = end_col,
        hl_group = segment.group,
      })
    end
  end
end

local function visible_row_ranges(buf)
  local ranges = {}
  for _, win in ipairs(vim.api.nvim_list_wins()) do
    if vim.api.nvim_win_is_valid(win) and vim.api.nvim_win_get_buf(win) == buf then
      local info = vim.fn.getwininfo(win)[1]
      if info and info.topline and info.botline then
        table.insert(ranges, {
          first = math.max(0, info.topline - 1),
          last = math.max(0, info.botline - 1),
        })
      end
    end
  end
  return ranges
end

local function buffer_windows(buf)
  local windows = {}
  for _, win in ipairs(vim.api.nvim_list_wins()) do
    if vim.api.nvim_win_is_valid(win) and vim.api.nvim_win_get_buf(win) == buf then
      table.insert(windows, win)
    end
  end
  return windows
end

local function save_win_view(win)
  local ok, view = pcall(vim.api.nvim_win_call, win, vim.fn.winsaveview)
  if ok then
    return view
  end
  return nil
end

local function save_buffer_views(buf)
  local views = {}
  for _, win in ipairs(buffer_windows(buf)) do
    views[win] = save_win_view(win)
  end
  return views
end

local function restore_win_view(win, view)
  if not view or not vim.api.nvim_win_is_valid(win) then
    return
  end

  pcall(vim.api.nvim_win_call, win, function()
    vim.fn.winrestview(view)
  end)
end

local function restore_buffer_views(views)
  for win, view in pairs(views or {}) do
    restore_win_view(win, view)
  end
end

local function save_current_view(buf)
  local state = RENDER_STATES[buf]
  if not state or vim.api.nvim_get_current_buf() ~= buf then
    return
  end
  state.view = vim.fn.winsaveview()
end

local function restore_current_view(buf)
  local state = RENDER_STATES[buf]
  if not state or not state.view or vim.api.nvim_get_current_buf() ~= buf then
    return
  end
  pcall(vim.fn.winrestview, state.view)
end

local function mirror_visible_treesitter(buf)
  local state = RENDER_STATES[buf]
  if not state or not vim.api.nvim_buf_is_valid(buf) then
    return
  end

  vim.api.nvim_buf_clear_namespace(buf, SOURCE_NAMESPACE, 0, -1)

  for _, range in ipairs(visible_row_ranges(buf)) do
    for review_row = range.first, range.last do
      local row = state.rows[review_row + 1]
      if row_is_mirrorable(row) then
        apply_line_segments(buf, review_row, row.code_start_col or 0, segments_for_row(state, row))
      end
    end
  end
end

local function schedule_visible_mirror(buf)
  local state = RENDER_STATES[buf]
  if not state or state.scheduled then
    return
  end

  state.scheduled = true
  vim.defer_fn(function()
    local current = RENDER_STATES[buf]
    if not current then
      return
    end
    current.scheduled = false
    mirror_visible_treesitter(buf)
  end, MIRROR_DEBOUNCE_MS)
end

local function setup_mirror_autocmds(buf)
  local state = RENDER_STATES[buf]
  if not state then
    return
  end

  if state.augroup then
    pcall(vim.api.nvim_del_augroup_by_id, state.augroup)
  end

  state.augroup = vim.api.nvim_create_augroup(AUTOCMD_GROUP_PREFIX .. buf, { clear = true })
  vim.api.nvim_create_autocmd(AUTOCMD_EVENTS, {
    group = state.augroup,
    callback = function()
      schedule_visible_mirror(buf)
    end,
  })
  vim.api.nvim_create_autocmd(VIEW_SAVE_EVENTS, {
    group = state.augroup,
    buffer = buf,
    callback = function()
      save_current_view(buf)
    end,
  })
  vim.api.nvim_create_autocmd("BufEnter", {
    group = state.augroup,
    buffer = buf,
    callback = function()
      restore_current_view(buf)
    end,
  })
  vim.api.nvim_create_autocmd("BufWipeout", {
    group = state.augroup,
    buffer = buf,
    callback = function()
      RENDER_STATES[buf] = nil
    end,
  })
end

local apply_render

local function close_composer(state)
  if state.composer_win and vim.api.nvim_win_is_valid(state.composer_win) then
    vim.api.nvim_win_close(state.composer_win, true)
  end
  if state.composer_buf and vim.api.nvim_buf_is_valid(state.composer_buf) then
    vim.api.nvim_buf_delete(state.composer_buf, { force = true })
  end
  state.composer_win = nil
  state.composer_buf = nil
end

local function composer_width(review_win)
  local available = vim.api.nvim_win_get_width(review_win) - COMPOSER_GUTTER_COL - 2
  return math.max(COMPOSER_MIN_WIDTH, math.min(COMPOSER_MAX_WIDTH, available))
end

local function composer_row()
  local winline = vim.fn.winline()
  if winline > COMPOSER_HEIGHT + 3 then
    return winline - COMPOSER_HEIGHT - 2
  end
  return winline
end

local function composer_config(review_win, title)
  return {
    relative = "win",
    win = review_win,
    row = composer_row(),
    col = COMPOSER_GUTTER_COL,
    width = composer_width(review_win),
    height = COMPOSER_HEIGHT,
    border = "rounded",
    title = title or COMPOSER_TITLE,
    style = "minimal",
  }
end

local function composer_body(buf)
  local lines = vim.api.nvim_buf_get_lines(buf, 0, -1, false)
  return vim.trim(table.concat(lines, "\n"))
end

local function apply_mutation_render(state, review_buf, render)
  close_composer(state)
  apply_render(state.root, review_buf, render, state.client_id)
end

local function confirm_invalidating(input)
  if not input or not input.invalidates_later_activity then
    return true
  end
  local choice = vim.fn.confirm(
    input.title .. "\n\n" .. input.message,
    COMMENT_CONFIRM_CHOICES,
    COMMENT_CONFIRM_DEFAULT,
    COMMENT_CONFIRM_DANGER
  )
  return choice == 1
end

local function submit_composer(review_buf, draft_buf, on_submit)
  local state = RENDER_STATES[review_buf]
  if not state then
    return
  end

  local body = composer_body(draft_buf)
  if body == "" then
    vim.notify(COMMENT_EMPTY_MESSAGE, vim.log.levels.WARN)
    return
  end

  on_submit(state, body)
end

local function open_composer(review_buf, opts)
  local state = RENDER_STATES[review_buf]
  if not state then
    return
  end

  close_composer(state)
  local review_win = vim.api.nvim_get_current_win()
  local draft_buf = vim.api.nvim_create_buf(false, true)
  vim.bo[draft_buf].buftype = "nofile"
  vim.bo[draft_buf].bufhidden = "wipe"
  vim.bo[draft_buf].buflisted = false
  vim.bo[draft_buf].swapfile = false
  vim.bo[draft_buf].filetype = COMPOSER_FILETYPE
  vim.api.nvim_buf_set_lines(draft_buf, 0, -1, false, vim.split(opts.initial_body or COMPOSER_INITIAL_LINE, "\n", {
    plain = true,
  }))

  local draft_win = vim.api.nvim_open_win(draft_buf, true, composer_config(review_win, opts.title))
  state.composer_buf = draft_buf
  state.composer_win = draft_win

  vim.keymap.set({ "n", "i" }, COMPOSER_SUBMIT_MAP, function()
    submit_composer(review_buf, draft_buf, opts.on_submit)
  end, { buffer = draft_buf, nowait = true })
  vim.keymap.set("n", COMPOSER_CANCEL_MAP, function()
    close_composer(state)
  end, { buffer = draft_buf, nowait = true })
  vim.keymap.set("n", "<Esc>", function()
    close_composer(state)
  end, { buffer = draft_buf, nowait = true })

  vim.cmd("startinsert")
end

function apply_render(root, buf, render, client_id)
  if not render or not render.lines then
    return
  end

  local existing = RENDER_STATES[buf]
  local visible_views = save_buffer_views(buf)
  local remembered_view = existing and existing.view or nil
  if existing then
    close_composer(existing)
  end

  render = mask_dirty_file_diffs(root, render)
  set_lines(buf, render.lines)
  apply_structural_highlights(buf, render.highlights)
  apply_diagnostics(buf, render.diagnostics)
  RENDER_STATES[buf] = {
    root = root,
    client_id = client_id,
    rows = render.rows or {},
    source_buffers = {},
    source_lsp_buffers = {},
    source_segments = {},
    scheduled = false,
    view = remembered_view,
  }
  restore_buffer_views(visible_views)
  setup_mirror_autocmds(buf)
  mirror_visible_treesitter(buf)
end

function M.comment_current(buf, anchor)
  buf = buf or vim.api.nvim_get_current_buf()
  local state = RENDER_STATES[buf]
  if not state then
    return
  end

  if anchor and anchor.scope == ROW_SCOPE_FILE and anchor.path then
    open_composer(buf, {
      on_submit = function(state, body)
        lsp.create_thread(state.client_id, buf, {
          scope = anchor.scope,
          path = anchor.path,
          body = body,
        }, function(render)
          apply_mutation_render(state, buf, render)
        end)
      end,
    })
    return
  end

  if anchor and anchor.scope == ROW_SCOPE_LINE and anchor.path and anchor.side and anchor.start_line then
    open_composer(buf, {
      on_submit = function(state, body)
        lsp.create_thread(state.client_id, buf, {
          scope = anchor.scope,
          path = anchor.path,
          side = anchor.side,
          start_line = anchor.start_line,
          end_line = anchor.end_line,
          body = body,
        }, function(render)
          apply_mutation_render(state, buf, render)
        end)
      end,
    })
    return
  end

  local cursor = vim.api.nvim_win_get_cursor(0)
  local row = state.rows[cursor[1]]
  if not row_is_commentable(row) then
    vim.notify(COMMENT_UNAVAILABLE_MESSAGE, vim.log.levels.WARN)
    return
  end

  open_composer(buf, {
    on_submit = function(state, body)
      lsp.create_thread(state.client_id, buf, {
        scope = row.scope or ROW_SCOPE_LINE,
        path = row.path,
        side = row.side,
        start_line = row.start_line or row.source_line,
        end_line = row.end_line or row.source_line,
        body = body,
      }, function(render)
        apply_mutation_render(state, buf, render)
      end)
    end,
  })
end

function M.reply_to_thread(buf, input)
  buf = buf or vim.api.nvim_get_current_buf()
  if not input or not input.thread_id then
    vim.notify(COMMENT_REPLY_UNAVAILABLE_MESSAGE, vim.log.levels.WARN)
    return
  end

  open_composer(buf, {
    on_submit = function(state, body)
      lsp.reply_to_thread(state.client_id, buf, {
        thread_id = input.thread_id,
        body = body,
      }, function(render)
        apply_mutation_render(state, buf, render)
      end)
    end,
  })
end

function M.edit_comment(buf, input)
  buf = buf or vim.api.nvim_get_current_buf()
  if not input or not input.comment_id then
    vim.notify(COMMENT_EDIT_UNAVAILABLE_MESSAGE, vim.log.levels.WARN)
    return
  end

  open_composer(buf, {
    initial_body = input.body or "",
    on_submit = function(state, body)
      if
        not confirm_invalidating({
          invalidates_later_activity = input.invalidates_later_activity,
          title = COMMENT_EDIT_CONFIRM_TITLE,
          message = COMMENT_EDIT_CONFIRM_MESSAGE,
        })
      then
        return
      end
      lsp.edit_comment(state.client_id, buf, {
        comment_id = input.comment_id,
        body = body,
      }, function(render)
        apply_mutation_render(state, buf, render)
      end)
    end,
  })
end

function M.delete_comment(buf, input)
  buf = buf or vim.api.nvim_get_current_buf()
  local state = RENDER_STATES[buf]
  if not state or not input or not input.comment_id then
    vim.notify(COMMENT_DELETE_UNAVAILABLE_MESSAGE, vim.log.levels.WARN)
    return
  end
  if
    not confirm_invalidating({
      invalidates_later_activity = input.invalidates_later_activity,
      title = COMMENT_DELETE_CONFIRM_TITLE,
      message = COMMENT_DELETE_CONFIRM_MESSAGE,
    })
  then
    return
  end
  lsp.delete_comment(state.client_id, buf, {
    comment_id = input.comment_id,
  }, function(render)
    apply_mutation_render(state, buf, render)
  end)
end

function M.resolve_thread(buf, input)
  buf = buf or vim.api.nvim_get_current_buf()
  local state = RENDER_STATES[buf]
  if not state or not input or not input.thread_id then
    vim.notify(COMMENT_THREAD_UNAVAILABLE_MESSAGE, vim.log.levels.WARN)
    return
  end
  lsp.resolve_thread(state.client_id, buf, {
    thread_id = input.thread_id,
  }, function(render)
    apply_mutation_render(state, buf, render)
  end)
end

function M.reopen_thread(buf, input)
  buf = buf or vim.api.nvim_get_current_buf()
  local state = RENDER_STATES[buf]
  if not state or not input or not input.thread_id then
    vim.notify(COMMENT_THREAD_UNAVAILABLE_MESSAGE, vim.log.levels.WARN)
    return
  end
  lsp.reopen_thread(state.client_id, buf, {
    thread_id = input.thread_id,
  }, function(render)
    apply_mutation_render(state, buf, render)
  end)
end

function M.is_review_buffer(buf)
  return RENDER_STATES[buf or vim.api.nvim_get_current_buf()] ~= nil
end

function M.open_source_at_cursor(buf)
  buf = buf or vim.api.nvim_get_current_buf()
  local state = RENDER_STATES[buf]
  if not state then
    return
  end

  local cursor = vim.api.nvim_win_get_cursor(0)
  local row = state.rows[cursor[1]]
  if not row_is_source_jumpable(row) then
    vim.notify(OPEN_SOURCE_UNAVAILABLE_MESSAGE, vim.log.levels.WARN)
    return
  end

  local full_path = file_buffer_name(state.root, row.path)
  if vim.fn.filereadable(full_path) ~= 1 then
    vim.notify(OPEN_SOURCE_MISSING_MESSAGE .. row.path, vim.log.levels.WARN)
    return
  end

  vim.cmd.edit(vim.fn.fnameescape(full_path))
  vim.bo.buflisted = true
  local line = math.min(row_jump_line(row), vim.api.nvim_buf_line_count(0))
  vim.api.nvim_win_set_cursor(0, { line, 0 })
end

function M.refresh_from_client(client_id)
  for buf, state in pairs(RENDER_STATES) do
    if state.client_id == client_id and vim.api.nvim_buf_is_valid(buf) then
      lsp.render_now(client_id, buf, function(render)
        apply_render(state.root, buf, render, client_id)
      end)
    end
  end
end

function M.refresh_all()
  for buf, state in pairs(RENDER_STATES) do
    if vim.api.nvim_buf_is_valid(buf) then
      lsp.render_now(state.client_id, buf, function(render)
        apply_render(state.root, buf, render, state.client_id)
      end)
    end
  end
end

function M.source_location(buf)
  buf = buf or vim.api.nvim_get_current_buf()
  local state = RENDER_STATES[buf]
  if not state then
    return nil
  end

  local cursor = vim.api.nvim_win_get_cursor(0)
  local row = state.rows[cursor[1]]
  if not row_is_proxyable(row) then
    return nil, SOURCE_PROXY_UNAVAILABLE
  end

  local source = source_for_proxy_row(state, row)
  if not source then
    return nil, SOURCE_PROXY_UNAVAILABLE
  end

  local source_row = row.source_line - 1
  local source_text = vim.api.nvim_buf_get_lines(source, source_row, source_row + 1, false)[1] or ""
  local source_col = math.max(0, cursor[2] - (row.code_start_col or 0))
  source_col = math.min(source_col, #source_text)

  return {
    bufnr = source,
    row = source_row,
    col = source_col,
    path = row.path,
    source_line = row.source_line,
  }
end

local function set_review_keymaps(buf)
  vim.keymap.set("n", OPEN_SOURCE_KEY, function()
    M.open_source_at_cursor(buf)
  end, {
    buffer = buf,
    desc = OPEN_SOURCE_DESC,
    nowait = true,
  })
end

function M.open(root, review_id, session)
  local name = BUFFER_PREFIX .. review_id
  local buf = existing_buffer(name)

  if buf then
    vim.api.nvim_set_current_buf(buf)
  else
    vim.cmd("enew")
    buf = vim.api.nvim_get_current_buf()
    vim.api.nvim_buf_set_name(buf, name)
  end

  set_review_options(buf)
  set_review_keymaps(buf)
  define_highlights()
  define_diff_gutter_highlights()
  set_lines(buf, {
    "Peers review " .. review_id,
    "",
    "Vox: " .. session.vox_url,
    "LSP: " .. session.nvim_lsp_url,
    "",
    "Try:",
    "  vim.lsp.buf.hover()",
    "  vim.lsp.buf.code_action()",
    "  vim.lsp.buf.document_symbol()",
  })

  local client_id = lsp.attach(buf, root, session)
  if client_id then
    lsp.render(client_id, buf, function(render)
      apply_render(root, buf, render, client_id)
    end)
  end
end

return M
