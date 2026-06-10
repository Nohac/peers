local lsp = require("peers.lsp")
local sidebar = require("peers.sidebar")
local source_decorations = require("peers.source_decorations")
local timing = require("peers.timing")

local M = {}

M._MARKDOWN_NAMESPACE = vim.api.nvim_create_namespace("peers-review-markdown")

local BUFFER_PREFIX = "peers://review/"
local FILETYPE = "peersdiff"
local NAMESPACE = vim.api.nvim_create_namespace("peers-review")
local SOURCE_NAMESPACE = vim.api.nvim_create_namespace("peers-review-source")
local DIAGNOSTIC_NAMESPACE = vim.api.nvim_create_namespace("peers-review-diagnostics")
local ADD_FALLBACK_FG = "#3fb950"
local DELETE_FALLBACK_FG = "#f85149"
local RESOLVED_FALLBACK_FG = "#3fb950"
local THREAD_FALLBACK_FG = "#58a6ff"
local THREAD_CONTEXT_FALLBACK_FG = "#d29922"
local THREAD_STALE_FALLBACK_FG = "#f85149"
local THREAD_DETACHED_FALLBACK_FG = "#8b949e"
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
local RESOLVED_FOREGROUND_GROUPS = {
  "GitSignsAdd",
  "Added",
  "DiagnosticOk",
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
local THREAD_CONTEXT_FOREGROUND_GROUPS = {
  "DiagnosticWarn",
  "WarningMsg",
  "GitSignsChange",
}
local THREAD_STALE_FOREGROUND_GROUPS = {
  "DiagnosticError",
  "ErrorMsg",
  "GitSignsDelete",
}
local THREAD_DETACHED_FOREGROUND_GROUPS = {
  "Comment",
  "LineNr",
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
local ROW_KIND_HUNK_HEADER = "hunk_header"
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
local COMPOSER_TITLE = " Comment "
local COMPOSER_FILETYPE = "markdown"
local COMPOSER_INITIAL_LINE = ""
local COMPOSER_SUBMIT_MAP = "<C-s>"
local COMPOSER_CANCEL_MAP = "q"
local COMPOSER_HEIGHT = 7
local COMPOSER_MIN_WIDTH = 40
local COMPOSER_MAX_WIDTH = 88
local COMPOSER_GUTTER_COL = 14
local PAUSED_REFRESH_CHECK_MS = 80
local COMMENT_EMPTY_MESSAGE = "Peers comment is empty"
local COMMENT_UNAVAILABLE_MESSAGE = "Peers comment is only available on diff lines for now"
local OPEN_SOURCE_UNAVAILABLE_MESSAGE = "Peers can only open source files from file, diff, or comment rows"
local OPEN_SOURCE_MISSING_MESSAGE = "Peers source file does not exist: "
local COMMENT_REPLY_UNAVAILABLE_MESSAGE = "Peers reply is only available on comment threads"
local COMMENT_EDIT_UNAVAILABLE_MESSAGE = "Peers edit is only available on editable comments"
local COMMENT_DELETE_UNAVAILABLE_MESSAGE = "Peers delete is only available on editable comments"
local COMMENT_THREAD_UNAVAILABLE_MESSAGE = "Peers thread action is only available on comment threads"
local COMMENT_COLLAPSE_UNAVAILABLE_MESSAGE = "Peers collapse is only available on comment threads"
local OPEN_SOURCE_KEY = "<CR>"
local COMMENT_CONFIRM_CHOICES = "&Proceed\n&Cancel"
local COMMENT_CONFIRM_DEFAULT = 2
local COMMENT_CONFIRM_DANGER = "WarningMsg"
local COMMENT_EDIT_CONFIRM_TITLE = "Edit comment?"
local COMMENT_EDIT_CONFIRM_MESSAGE = "Editing this comment will remove later replies and thread status changes from the visible review state."
local COMMENT_DELETE_CONFIRM_TITLE = "Delete comment?"
local COMMENT_DELETE_CONFIRM_MESSAGE = "Deleting this comment will remove later replies and thread status changes from the visible review state."
local MIRROR_DEBOUNCE_MS = 30
local MIRROR_BATCH_BUDGET_MS = 8
local MIRROR_BATCH_MIN_ROWS = 8
local AUTOCMD_GROUP_PREFIX = "peers-review-source-"
local SOURCE_CHANGE_AUGROUP = "peers-review-source-changes"
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
local SOURCE_CHANGE_EVENTS = {
  "TextChanged",
  "TextChangedI",
  "TextChangedP",
  "BufWritePost",
}
local CACHE_KEY_SEPARATOR = ":"
local UNKNOWN_FILE_TIME = -1
local UNKNOWN_FILE_SIZE = -1
local SOURCE_HELPER_BUFFER_VAR = "peers_source_helper"
local SOURCE_HELPER_SIGNATURE_VAR = "peers_source_signature"
local SOURCE_SYNTAX_READY_VAR = "peers_source_syntax_ready"
local RENDER_STATES = {}

M._UI_STATE_FILE = "nvim-ui.json"
M._UI_STATE_COLLAPSED_FILES = "collapsed_files"
M._FILE_COLLAPSE_UNAVAILABLE_MESSAGE = "Peers file collapse is only available on file rows"

local function source_tree_priority()
  return (vim.hl and vim.hl.priorities and vim.hl.priorities.user or 200) + 10
end

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

function M._configure_review_window(win)
  if not win or not vim.api.nvim_win_is_valid(win) then
    return
  end
  vim.wo[win].foldmethod = "manual"
  vim.wo[win].foldenable = true
  vim.wo[win].foldcolumn = "0"
end

function M._configure_review_windows(buf)
  for _, win in ipairs(vim.fn.win_findbuf(buf)) do
    M._configure_review_window(win)
  end
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
  local thread_context_fg = foreground_from(THREAD_CONTEXT_FOREGROUND_GROUPS, THREAD_CONTEXT_FALLBACK_FG)
  local thread_stale_fg = foreground_from(THREAD_STALE_FOREGROUND_GROUPS, THREAD_STALE_FALLBACK_FG)
  local thread_detached_fg = foreground_from(THREAD_DETACHED_FOREGROUND_GROUPS, THREAD_DETACHED_FALLBACK_FG)
  local thread_resolved_fg = foreground_from(RESOLVED_FOREGROUND_GROUPS, RESOLVED_FALLBACK_FG)
  pcall(vim.api.nvim_set_hl, 0, "PeersDiffThreadBorderContext", {
    default = true,
    [HIGHLIGHT_FG] = thread_context_fg,
  })
  pcall(vim.api.nvim_set_hl, 0, "PeersDiffThreadBorderStale", {
    default = true,
    [HIGHLIGHT_FG] = thread_stale_fg,
  })
  pcall(vim.api.nvim_set_hl, 0, "PeersDiffThreadBorderDetached", {
    default = true,
    [HIGHLIGHT_FG] = thread_detached_fg,
  })
  pcall(vim.api.nvim_set_hl, 0, "PeersDiffThreadResolved", {
    default = true,
    [HIGHLIGHT_FG] = thread_resolved_fg,
    [HIGHLIGHT_BOLD] = true,
  })
  pcall(vim.api.nvim_set_hl, 0, "PeersCommitSummaryAdded", {
    default = true,
    [HIGHLIGHT_FG] = add_fg,
    [HIGHLIGHT_BOLD] = true,
  })
  pcall(vim.api.nvim_set_hl, 0, "PeersCommitSummaryRemoved", {
    default = true,
    [HIGHLIGHT_FG] = delete_fg,
    [HIGHLIGHT_BOLD] = true,
  })
  pcall(vim.api.nvim_set_hl, 0, "PeersCommitSummaryPositive", {
    default = true,
    [HIGHLIGHT_FG] = add_fg,
    [HIGHLIGHT_BOLD] = true,
  })
  pcall(vim.api.nvim_set_hl, 0, "PeersCommitSummaryNegative", {
    default = true,
    [HIGHLIGHT_FG] = delete_fg,
    [HIGHLIGHT_BOLD] = true,
  })
  pcall(vim.api.nvim_set_hl, 0, "PeersCommitSummaryNeutral", {
    default = true,
    [HIGHLIGHT_FG] = thread_detached_fg,
  })
  pcall(vim.api.nvim_set_hl, 0, "PeersCommitSummaryOpen", {
    default = true,
    [HIGHLIGHT_FG] = thread_fg,
    [HIGHLIGHT_BOLD] = true,
  })
  pcall(vim.api.nvim_set_hl, 0, "PeersCommitSummaryClosed", {
    default = true,
    [HIGHLIGHT_FG] = thread_resolved_fg,
    [HIGHLIGHT_BOLD] = true,
  })
  pcall(vim.api.nvim_set_hl, 0, "PeersCommitSummarySeparator", {
    default = true,
    [HIGHLIGHT_FG] = thread_detached_fg,
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
  pcall(vim.api.nvim_set_hl, 0, "PeersDiffThreadLocationNote", {
    default = true,
    [HIGHLIGHT_FG] = thread_detached_fg,
  })
  pcall(vim.api.nvim_set_hl, 0, "PeersDiffThreadRail", {
    default = true,
    [HIGHLIGHT_FG] = thread_fg,
    [HIGHLIGHT_BG] = thread_fg,
  })
  pcall(vim.api.nvim_set_hl, 0, "PeersDiffThreadRailContext", {
    default = true,
    [HIGHLIGHT_FG] = thread_context_fg,
    [HIGHLIGHT_BG] = thread_context_fg,
  })
  pcall(vim.api.nvim_set_hl, 0, "PeersDiffThreadRailStale", {
    default = true,
    [HIGHLIGHT_FG] = thread_stale_fg,
    [HIGHLIGHT_BG] = thread_stale_fg,
  })
  pcall(vim.api.nvim_set_hl, 0, "PeersDiffThreadRailDetached", {
    default = true,
    [HIGHLIGHT_FG] = thread_detached_fg,
    [HIGHLIGHT_BG] = thread_detached_fg,
  })
end

local function set_lines(buf, lines)
  vim.bo[buf].readonly = false
  vim.bo[buf].modifiable = true
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
  vim.bo[buf].modifiable = false
  vim.bo[buf].readonly = true
end

local function set_line_range(buf, first, last, lines)
  vim.bo[buf].readonly = false
  vim.bo[buf].modifiable = true
  vim.api.nvim_buf_set_lines(buf, first, last, false, lines)
  vim.bo[buf].modifiable = false
  vim.bo[buf].readonly = true
end

function M._ui_state_path(root)
  return root .. "/.peers/" .. M._UI_STATE_FILE
end

function M._sorted_keys(map)
  local keys = {}
  for key, value in pairs(map or {}) do
    if value then
      table.insert(keys, key)
    end
  end
  table.sort(keys)
  return keys
end

function M._load_collapsed_files(root)
  local path = M._ui_state_path(root)
  local ok_read, lines = pcall(vim.fn.readfile, path)
  if not ok_read then
    return {}
  end
  if not lines or #lines == 0 then
    return {}
  end
  local ok, decoded = pcall(vim.json.decode, table.concat(lines, "\n"))
  if not ok or type(decoded) ~= "table" or type(decoded[M._UI_STATE_COLLAPSED_FILES]) ~= "table" then
    return {}
  end
  local collapsed = {}
  for _, file_path in ipairs(decoded[M._UI_STATE_COLLAPSED_FILES]) do
    if type(file_path) == "string" and file_path ~= "" then
      collapsed[file_path] = true
    end
  end
  return collapsed
end

function M._save_collapsed_files(root, collapsed)
  local peers_dir = root .. "/.peers"
  vim.fn.mkdir(peers_dir, "p")
  local encoded = vim.json.encode({
    [M._UI_STATE_COLLAPSED_FILES] = M._sorted_keys(collapsed),
  })
  vim.fn.writefile({ encoded }, M._ui_state_path(root))
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
    sidebar_counts = render.sidebar_counts,
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

local function apply_structural_highlights(buf, highlights, rows)
  vim.api.nvim_buf_clear_namespace(buf, NAMESPACE, 0, -1)
  vim.api.nvim_buf_clear_namespace(buf, M._MARKDOWN_NAMESPACE, 0, -1)
  for _, highlight in ipairs(highlights or {}) do
    vim.api.nvim_buf_set_extmark(buf, NAMESPACE, highlight.line, highlight.start_col, {
      end_col = highlight.end_col,
      hl_group = highlight.group,
    })
  end
  M._apply_comment_markdown_highlights(buf, rows, highlights)
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

local function ensure_source_runtime(buf, full_path)
  if vim.bo[buf].filetype == "" then
    local filetype = vim.filetype.match({ filename = full_path })
    if filetype then
      vim.bo[buf].filetype = filetype
    end
  end

  if not vim.b[buf][SOURCE_SYNTAX_READY_VAR] then
    pcall(vim.api.nvim_buf_call, buf, function()
      pcall(vim.cmd, "silent! syntax enable")
    end)
    vim.b[buf][SOURCE_SYNTAX_READY_VAR] = true
  end
end

local function ensure_source_signature(root, path, buf)
  local signature = source_file_signature(file_buffer_name(root, path))
  reload_source_helper_if_stale(buf, signature)
  vim.b[buf][SOURCE_HELPER_SIGNATURE_VAR] = signature
  return table.concat({
    signature,
    vim.api.nvim_buf_get_changedtick(buf),
  }, CACHE_KEY_SEPARATOR)
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

  ensure_source_runtime(buf, full_path)

  return buf
end

local function ensure_highlighter(buf)
  if vim.bo[buf].filetype == "" then
    return false
  end

  local ok_start = pcall(vim.treesitter.start, buf)
  if not ok_start then
    return false
  end

  local ok_parse = pcall(function()
    local parser = vim.treesitter.get_parser(buf)
    parser:parse(true)
  end)
  return ok_parse
end

local function push_group(target, seen, group)
  if group and group ~= "" and not seen[group] then
    seen[group] = true
    table.insert(target, group)
  end
end

local function push_inspected_group(target, seen, item)
  push_group(target, seen, item and item.hl_group)
  push_group(target, seen, item and item.hl_group_link)
end

local function inspect_source_pos(buf, row, col)
  if vim.inspect_pos then
    local ok, inspected = pcall(vim.inspect_pos, buf, row, col, {
      syntax = true,
      treesitter = true,
      semantic_tokens = true,
      extmarks = false,
    })
    if ok then
      return inspected
    end
  end

  return nil
end

local function inspected_source_groups_at(buf, row, col)
  local groups = {}
  local seen = {}
  local inspected = inspect_source_pos(buf, row, col)
  if not inspected then
    return groups
  end

  for _, item in ipairs(inspected.syntax or {}) do
    push_inspected_group(groups, seen, item)
  end
  for _, item in ipairs(inspected.treesitter or {}) do
    push_inspected_group(groups, seen, item)
  end
  for _, item in ipairs(inspected.semantic_tokens or {}) do
    push_inspected_group(groups, seen, item.opts or item)
  end
  return groups
end

local function highlight_groups_key(groups)
  return table.concat(groups, "\0")
end

local function source_line_segments(source_buf, source_line, groups_at)
  local source_row = source_line - 1
  local source_text = vim.api.nvim_buf_get_lines(source_buf, source_row, source_row + 1, false)[1]
  if not source_text or source_text == "" then
    return {}
  end

  local segments = {}
  local active_groups = {}
  local active_key = ""
  local active_start = nil
  local byte_len = #source_text

  for col = 0, byte_len do
    local groups = col < byte_len and groups_at(source_buf, source_row, col) or {}
    local key = highlight_groups_key(groups)
    if key ~= active_key then
      if #active_groups > 0 and active_start and active_start < col then
        table.insert(segments, {
          start_col = active_start,
          end_col = col,
          groups = active_groups,
        })
      end
      active_groups = groups
      active_key = key
      active_start = col
    end
  end

  return segments
end

function M._markdown_source_buffer(lines)
  local buf = M._markdown_buf
  if not buf or not vim.api.nvim_buf_is_valid(buf) then
    buf = vim.api.nvim_create_buf(false, true)
    M._markdown_buf = buf
    vim.bo[buf].buftype = "nofile"
    vim.bo[buf].bufhidden = "hide"
    vim.bo[buf].buflisted = false
    vim.bo[buf].swapfile = false
    vim.bo[buf].filetype = COMPOSER_FILETYPE
    vim.bo[buf].textwidth = 0
    vim.bo[buf].wrapmargin = 0
    vim.bo[buf].formatoptions = (vim.bo[buf].formatoptions or ""):gsub("[tcroa]", "")
    ensure_source_runtime(buf, "comment.md")
  end

  vim.bo[buf].modifiable = true
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
  vim.bo[buf].modified = false
  vim.bo[buf].modifiable = false
  ensure_highlighter(buf)
  return buf
end

function M._comment_body_start_col(line)
  local _, prefix_end = line:find("│ ", 1, true)
  return prefix_end or 0
end

function M._highlight_groups_by_line(highlights, line_offset)
  local by_line = {}
  line_offset = line_offset or 0
  for _, highlight in ipairs(highlights or {}) do
    local line = line_offset + highlight.line
    by_line[line] = by_line[line] or {}
    by_line[line][highlight.group] = true
    if highlight.group == "PeersDiffThreadBody" then
      by_line[line].body_start_col = highlight.start_col
      by_line[line].body_end_col = highlight.end_col
    end
  end
  return by_line
end

function M._comment_markdown_blocks(buf, rows, highlights, line_offset)
  local by_line = M._highlight_groups_by_line(highlights, line_offset)
  local blocks = {}
  local by_comment = {}
  for row_index, row in ipairs(rows or {}) do
    local line = (line_offset or 0) + row_index - 1
    if row.kind == ROW_KIND_COMMENT and row.comment_id and row.comment_body_line ~= nil then
      local text = vim.api.nvim_buf_get_lines(buf, line, line + 1, false)[1] or ""
      local start_col = by_line[line] and by_line[line].body_start_col or M._comment_body_start_col(text)
      local end_col = by_line[line] and by_line[line].body_end_col or math.min(#text, start_col + #row.comment_body_line)
      local block = by_comment[row.comment_id]
      if not block then
        block = {
          entries = {},
          lines = {},
        }
        by_comment[row.comment_id] = block
        table.insert(blocks, block)
      end
      table.insert(block.entries, {
        line = line,
        start_col = start_col,
        end_col = end_col,
      })
      table.insert(block.lines, row.comment_body_line)
    end
  end
  return blocks
end

function M._apply_comment_markdown_block(buf, block)
  if not block or #block.entries == 0 then
    return
  end
  local source = M._markdown_source_buffer(block.lines)
  local priority = 5000
  for index, entry in ipairs(block.entries) do
    local segments = source_line_segments(source, index, inspected_source_groups_at)
    for _, segment in ipairs(segments) do
      local segment_start = math.min(entry.end_col, entry.start_col + segment.start_col)
      local segment_end = math.min(entry.end_col, entry.start_col + segment.end_col)
      if segment_start < segment_end then
        for priority_offset, group in ipairs(segment.groups or {}) do
          vim.api.nvim_buf_set_extmark(buf, M._MARKDOWN_NAMESPACE, entry.line, segment_start, {
            end_col = segment_end,
            hl_group = group,
            priority = priority + priority_offset,
          })
        end
      end
    end
  end
end

function M._apply_comment_markdown_highlights(buf, rows, highlights, line_offset)
  for _, block in ipairs(M._comment_markdown_blocks(buf, rows, highlights, line_offset)) do
    M._apply_comment_markdown_block(buf, block)
  end
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

local function thread_line_label(row)
  local path = row.path or "review"
  local start_line = row.source_start_line or row.source_line
  local end_line = row.source_line or row.source_start_line
  if start_line and end_line and start_line ~= end_line then
    return string.format("%s:%d-%d", path, start_line, end_line)
  end
  if end_line then
    return string.format("%s:%d", path, end_line)
  end
  if start_line then
    return string.format("%s:%d", path, start_line)
  end
  return path
end

function M._source_range_label(path, start_line, end_line)
  path = path or "review"
  if start_line and end_line and start_line ~= end_line then
    return string.format("%s:%d-%d", path, start_line, end_line)
  end
  if end_line then
    return string.format("%s:%d", path, end_line)
  end
  if start_line then
    return string.format("%s:%d", path, start_line)
  end
  return path
end

function M._composer_title(kind, target)
  if target and target ~= "" then
    return string.format(" %s · %s ", kind, target)
  end
  return string.format(" %s ", kind)
end

function M._composer_line_title(action, path, start_line, end_line)
  local kind = "Line " .. action
  if start_line and end_line and start_line ~= end_line then
    kind = "Range " .. action
  end
  return M._composer_title(kind, M._source_range_label(path, start_line, end_line))
end

function M._composer_row_title(action, row)
  if not row then
    return M._composer_title(action)
  end
  if row.kind == ROW_KIND_FILE_HEADER then
    return M._composer_title("File " .. action, row.path)
  end
  return M._composer_line_title(
    action,
    row.path,
    row.source_start_line or row.source_line,
    row.source_line or row.source_start_line
  )
end

function M._composer_reply_title(row)
  if not row then
    return M._composer_title("Reply")
  end
  return M._composer_title(
    "Reply",
    M._source_range_label(
      row.path,
      row.source_start_line or row.source_line,
      row.source_line or row.source_start_line
    )
  )
end

function M._composer_anchor_title(action, anchor)
  if not anchor then
    return M._composer_title(action)
  end
  if anchor.scope == ROW_SCOPE_FILE then
    return M._composer_title("File " .. action, anchor.path)
  end
  return M._composer_line_title(action, anchor.path, anchor.start_line, anchor.end_line)
end

local function thread_scope_for_row(row)
  if row.source_line then
    return ROW_SCOPE_LINE
  end
  if row.path then
    return ROW_SCOPE_FILE
  end
  return "review"
end

local function thread_render_context(row)
  if not row then
    return nil
  end
  return {
    scope = thread_scope_for_row(row),
    path = row.path,
    line_label = thread_line_label(row),
    side = row.side,
    start_line = row.source_start_line or row.source_line,
    end_line = row.source_line or row.source_start_line,
    anchor_placement = row.anchor_placement,
  }
end

function M._input_thread_context(buf, input)
  local row = input and input.row or current_review_row(buf)
  return thread_render_context(row)
end

local function source_for_proxy_row(state, row)
  local source = state.source_lsp_buffers[row.path]
  if source ~= nil and source ~= false and not vim.api.nvim_buf_is_valid(source) then
    state.source_lsp_buffers[row.path] = nil
    source = nil
  end
  if source == nil then
    source = source_buffer(state.root, row.path)
    state.source_lsp_buffers[row.path] = source or false
  elseif source ~= false then
    ensure_source_signature(state.root, row.path, source)
  end

  if source == false then
    return nil
  end
  return source
end

local function source_for_row(state, row)
  local source = state.source_buffers[row.path]
  local signature = nil
  if source ~= nil and source ~= false and not vim.api.nvim_buf_is_valid(source) then
    state.source_buffers[row.path] = nil
    source = nil
  end
  if source == nil then
    source = source_buffer(state.root, row.path)
    if source and not ensure_highlighter(source) then
      source = false
    end
    state.source_buffers[row.path] = source or false
    if source then
      signature = ensure_source_signature(state.root, row.path, source)
    end
  elseif source ~= false then
    signature = ensure_source_signature(state.root, row.path, source)
  end

  if source == false then
    return nil
  end
  return source, signature
end

local function cache_for_file(state, row)
  local source, signature = source_for_row(state, row)
  if not source then
    return nil, nil
  end

  local file_cache = state.source_segments[row.path]
  if not file_cache or file_cache.signature ~= signature then
    if not ensure_highlighter(source) then
      return nil, nil
    end
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

  lines[row.source_line] = source_line_segments(source, row.source_line, inspected_source_groups_at)
  return lines[row.source_line]
end

local schedule_visible_mirror

local function apply_line_segments(buf, review_row, code_start_col, segments, base_priority)
  local line = vim.api.nvim_buf_get_lines(buf, review_row, review_row + 1, false)[1]
  if not line then
    return
  end

  local line_len = #line
  base_priority = base_priority or (vim.hl and vim.hl.priorities and vim.hl.priorities.treesitter or 100)
  for _, segment in ipairs(segments) do
    local start_col = math.min(line_len, code_start_col + segment.start_col)
    local end_col = math.min(line_len, code_start_col + segment.end_col)
    if start_col < end_col then
      for priority_offset, group in ipairs(segment.groups or {}) do
        vim.api.nvim_buf_set_extmark(buf, SOURCE_NAMESPACE, review_row, start_col, {
          end_col = end_col,
          hl_group = group,
          priority = base_priority + priority_offset,
        })
      end
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

local function save_win_view(win)
  local ok, view = pcall(vim.api.nvim_win_call, win, vim.fn.winsaveview)
  if ok then
    return view
  end
  return nil
end

local function restore_win_view(win, view)
  if not view or not vim.api.nvim_win_is_valid(win) then
    return
  end

  pcall(vim.api.nvim_win_call, win, function()
    vim.fn.winrestview(view)
  end)
end

local clamp_cursor

local function review_window_for(buf)
  local current = vim.api.nvim_get_current_win()
  if vim.api.nvim_win_is_valid(current) and vim.api.nvim_win_get_buf(current) == buf then
    return current
  end
  local wins = vim.fn.win_findbuf(buf)
  return wins[1]
end

local function review_win_for_row(buf, line)
  local win = review_window_for(buf)
  if not win or not vim.api.nvim_win_is_valid(win) then
    return nil
  end
  if line then
    local clamped_line, col = clamp_cursor(buf, line, 0)
    pcall(vim.api.nvim_win_set_cursor, win, { clamped_line, col })
  end
  return win
end

local function focus_review_row(buf, line)
  local win = review_win_for_row(buf, line)
  if win then
    vim.api.nvim_set_current_win(win)
  end
  return win
end

local function row_thread_offset(rows, index, thread_id)
  local first = index
  while first > 1 do
    local previous = rows[first - 1]
    if not previous or previous.thread_id ~= thread_id then
      break
    end
    first = first - 1
  end
  return index - first
end

local function semantic_anchor_for_view(rows, view)
  local line = math.max(1, view and view.lnum or 1)
  local col = math.max(0, view and view.col or 0)
  local row = rows and rows[line] or nil
  local anchor = {
    view = view,
    fallback_line = line,
    fallback_col = col,
    top_delta = line - math.max(1, view and view.topline or line),
  }

  if not row then
    return anchor
  end

  anchor.kind = row.kind
  anchor.path = row.path
  anchor.side = row.side
  anchor.source_line = row.source_line
  anchor.thread_id = row.thread_id
  anchor.comment_id = row.comment_id

  if row.thread_id then
    anchor.thread_offset = row_thread_offset(rows, line, row.thread_id)
  end

  return anchor
end

local function save_buffer_cursor_anchors(buf)
  local state = RENDER_STATES[buf]
  local anchors = {}
  for _, win in ipairs(vim.fn.win_findbuf(buf)) do
    local view = save_win_view(win)
    anchors[win] = semantic_anchor_for_view(state and state.rows or nil, view)
  end
  return anchors
end

local function find_row(rows, predicate)
  for index, row in ipairs(rows or {}) do
    if predicate(row) then
      return index, row
    end
  end
  return nil, nil
end

local function find_thread_row(rows, thread_id, offset)
  local first = find_row(rows, function(row)
    return row.thread_id == thread_id
  end)
  if not first then
    return nil, nil
  end

  local target = first + (offset or 0)
  local row = rows[target]
  if row and row.thread_id == thread_id then
    return target, row
  end
  return first, rows[first]
end

local function find_semantic_row(rows, anchor)
  if not anchor then
    return nil, nil
  end

  if anchor.comment_id then
    local index, row = find_row(rows, function(candidate)
      return candidate.comment_id == anchor.comment_id
    end)
    if index then
      return index, row
    end
  end

  if anchor.thread_id then
    local index, row = find_thread_row(rows, anchor.thread_id, anchor.thread_offset)
    if index then
      return index, row
    end
  end

  if anchor.path and anchor.side and anchor.source_line then
    local index, row = find_row(rows, function(candidate)
      return candidate.path == anchor.path and candidate.side == anchor.side and candidate.source_line == anchor.source_line
    end)
    if index then
      return index, row
    end
  end

  if anchor.path and (anchor.kind == ROW_KIND_FILE_HEADER or anchor.kind == ROW_KIND_HUNK_HEADER) then
    local index, row = find_row(rows, function(candidate)
      return candidate.path == anchor.path and candidate.kind == anchor.kind
    end)
    if index then
      return index, row
    end
  end

  if anchor.path then
    return find_row(rows, function(candidate)
      return candidate.path == anchor.path
    end)
  end

  return nil, nil
end

local function cursor_col_for_anchor(anchor)
  return anchor and anchor.fallback_col or 0
end

clamp_cursor = function(buf, line, col)
  local line_count = math.max(1, vim.api.nvim_buf_line_count(buf))
  line = math.max(1, math.min(line or 1, line_count))
  local text = vim.api.nvim_buf_get_lines(buf, line - 1, line, false)[1] or ""
  col = math.max(0, math.min(col or 0, #text))
  return line, col
end

function M._file_block_range(rows, path)
  if not path then
    return nil, nil
  end
  local first = nil
  for index, row in ipairs(rows or {}) do
    if row.kind == ROW_KIND_FILE_HEADER and row.path == path then
      first = index
      break
    end
  end
  if not first then
    return nil, nil
  end
  local last = #rows
  for index = first + 1, #rows do
    local row = rows[index]
    if row and row.kind == ROW_KIND_FILE_HEADER then
      last = index - 1
      break
    end
  end
  return first, last
end

function M._row_file_path(row)
  if row and row.path then
    return row.path
  end
  return nil
end

function M._apply_file_folds(buf, state)
  if not state or not state.rows then
    return
  end
  local ranges = {}
  for path in pairs(state.collapsed_files or {}) do
    if not (state.temporarily_expanded_files and state.temporarily_expanded_files[path]) then
      local first, last = M._file_block_range(state.rows, path)
      if first and last and last > first then
        table.insert(ranges, { first = first, last = last })
      end
    end
  end
  table.sort(ranges, function(left, right)
    return left.first < right.first
  end)

  for _, win in ipairs(vim.fn.win_findbuf(buf)) do
    if vim.api.nvim_win_is_valid(win) then
      local cursor = vim.api.nvim_win_get_cursor(win)
      local target_cursor = cursor
      for _, range in ipairs(ranges) do
        if cursor[1] >= range.first and cursor[1] <= range.last then
          target_cursor = { range.first, 0 }
          break
        end
      end
      vim.api.nvim_win_call(win, function()
        M._configure_review_window(win)
        vim.cmd("silent! normal! zE")
        for _, range in ipairs(ranges) do
          vim.cmd(string.format("silent! %d,%dfold", range.first, range.last))
          pcall(vim.api.nvim_win_set_cursor, win, { range.first, 0 })
          vim.cmd("silent! normal! zc")
        end
      end)
      local line, col = clamp_cursor(buf, target_cursor[1], target_cursor[2])
      pcall(vim.api.nvim_win_set_cursor, win, { line, col })
    end
  end
end

function M._temporarily_expand_file(buf, path)
  local state = RENDER_STATES[buf]
  if not state or not path or not (state.collapsed_files and state.collapsed_files[path]) then
    return
  end
  state.temporarily_expanded_files = state.temporarily_expanded_files or {}
  state.temporarily_expanded_files[path] = true
  M._apply_file_folds(buf, state)
end

function M._collapse_temporary_files_outside_cursor(buf)
  local state = RENDER_STATES[buf]
  if not state or not state.temporarily_expanded_files or not state.rows then
    return
  end
  local win = review_window_for(buf)
  if not win or not vim.api.nvim_win_is_valid(win) then
    return
  end
  local cursor = vim.api.nvim_win_get_cursor(win)
  local row = state.rows[cursor[1]]
  local current_path = row and row.path or nil
  local changed = false
  for path in pairs(state.temporarily_expanded_files) do
    if path ~= current_path then
      state.temporarily_expanded_files[path] = nil
      changed = true
    end
  end
  if changed then
    M._apply_file_folds(buf, state)
  end
end

local function restore_relative_win_view(win, buf, anchor)
  if not anchor or not anchor.view or not vim.api.nvim_win_is_valid(win) then
    return
  end

  local top_delta = math.max(0, anchor.top_delta or 0)
  local line = (anchor.view.topline or 1) + top_delta
  local col = cursor_col_for_anchor(anchor)
  line, col = clamp_cursor(buf, line, col)

  local view = vim.deepcopy(anchor.view)
  view.lnum = line
  view.col = col
  view.curswant = col
  view.topline = math.max(1, line - top_delta)

  pcall(vim.api.nvim_win_call, win, function()
    vim.fn.winrestview(view)
    pcall(vim.api.nvim_win_set_cursor, win, { line, col })
  end)
end

local function restore_semantic_win_view(win, buf, anchor, rows)
  if not anchor or not vim.api.nvim_win_is_valid(win) then
    return
  end

  local line = find_semantic_row(rows, anchor)
  if not line then
    restore_relative_win_view(win, buf, anchor)
    return
  end

  local col = cursor_col_for_anchor(anchor)
  line, col = clamp_cursor(buf, line, col)
  local view = vim.deepcopy(anchor.view or {})
  view.lnum = line
  view.col = col
  view.curswant = col
  view.topline = math.max(1, line - math.max(0, anchor.top_delta or 0))

  pcall(vim.api.nvim_win_call, win, function()
    vim.fn.winrestview(view)
    pcall(vim.api.nvim_win_set_cursor, win, { line, col })
  end)
end

local function restore_buffer_cursor_anchors(buf, anchors, rows)
  for win, anchor in pairs(anchors or {}) do
    restore_semantic_win_view(win, buf, anchor, rows)
  end
end

local function save_current_view(buf)
  local state = RENDER_STATES[buf]
  if not state or vim.api.nvim_get_current_buf() ~= buf then
    return
  end
  state.view = vim.fn.winsaveview()
end

function M._save_window_view(buf, win)
  local state = RENDER_STATES[buf]
  if not state or not win or not vim.api.nvim_win_is_valid(win) or vim.api.nvim_win_get_buf(win) ~= buf then
    return
  end
  state.view = save_win_view(win)
end

local function restore_current_view(buf)
  local state = RENDER_STATES[buf]
  if not state or not state.view or vim.api.nvim_get_current_buf() ~= buf then
    return
  end
  pcall(vim.fn.winrestview, state.view)
end

local function path_is_under(root, path)
  local normalized_root = vim.fn.fnamemodify(root, ":p")
  local normalized_path = vim.fn.fnamemodify(path, ":p")
  return normalized_path:sub(1, #normalized_root) == normalized_root
end

local function visible_mirror_rows(buf, state)
  local rows = {}
  for _, range in ipairs(visible_row_ranges(buf)) do
    for review_row = range.first, range.last do
      local row = state.rows[review_row + 1]
      if row_is_mirrorable(row) then
        table.insert(rows, review_row)
      end
    end
  end
  return rows
end

local function mirror_row(buf, state, review_row)
  local row = state.rows[review_row + 1]
  if not row_is_mirrorable(row) then
    return false
  end
  vim.api.nvim_buf_clear_namespace(buf, SOURCE_NAMESPACE, review_row, review_row + 1)
  apply_line_segments(buf, review_row, row.code_start_col or 0, segments_for_row(state, row), source_tree_priority())
  return true
end

local function run_mirror_batch(buf)
  local state = RENDER_STATES[buf]
  if not state or not vim.api.nvim_buf_is_valid(buf) then
    return
  end

  local batch = state.mirror_batch
  if not batch then
    state.mirror_scheduled = false
    return
  end

  local start = timing.now()
  local mirrored = 0
  while batch.index <= #batch.rows do
    if mirror_row(buf, state, batch.rows[batch.index]) then
      mirrored = mirrored + 1
    end
    batch.index = batch.index + 1
    if mirrored >= MIRROR_BATCH_MIN_ROWS and timing.ms(start) >= MIRROR_BATCH_BUDGET_MS then
      break
    end
  end

  if batch.index <= #batch.rows then
    vim.schedule(function()
      run_mirror_batch(buf)
    end)
    return
  end

  state.mirror_batch = nil
  state.mirror_scheduled = false
  if state.mirror_again then
    state.mirror_again = false
    schedule_visible_mirror(buf)
  end
  timing.log(state.root, "buffer", string.format(
    "mirror_visible_highlights_async %.1fms rows=%d buf=%s",
    timing.ms(batch.start),
    batch.total,
    tostring(buf)
  ))
end

schedule_visible_mirror = function(buf)
  local state = RENDER_STATES[buf]
  if not state then
    return
  end
  if state.mirror_scheduled then
    state.mirror_again = true
    return
  end

  state.mirror_scheduled = true
  vim.defer_fn(function()
    local current = RENDER_STATES[buf]
    if not current then
      return
    end
    current.mirror_batch = {
      rows = visible_mirror_rows(buf, current),
      index = 1,
      total = 0,
      start = timing.now(),
    }
    current.mirror_batch.total = #current.mirror_batch.rows
    run_mirror_batch(buf)
  end, MIRROR_DEBOUNCE_MS)
end

local apply_render
local apply_thread_patch
local schedule_pending_refresh_check

local function review_buffer_is_visible(buf)
  return #vim.fn.win_findbuf(buf) > 0
end

local function composer_is_open(state)
  return state
    and (
      (state.composer_win and vim.api.nvim_win_is_valid(state.composer_win))
      or (state.composer_buf and vim.api.nvim_buf_is_valid(state.composer_buf))
    )
end

local function review_refresh_pause_reason(state)
  if composer_is_open(state) then
    return "composer"
  end
  if vim.fn.pumvisible() == 1 then
    return "popup menu"
  end
  return nil
end

local function review_refresh_is_paused(state)
  return review_refresh_pause_reason(state) ~= nil
end

local function render_review_now(buf, state)
  if not vim.api.nvim_buf_is_valid(buf) then
    return
  end

  if state.render_in_flight then
    state.pending_refresh = true
    state.render_again = true
    timing.log(state.root, "buffer", "render_review_now coalesced buf=" .. tostring(buf))
    return
  end

  local start = timing.now()
  timing.log(state.root, "buffer", "render_review_now start buf=" .. tostring(buf))
  state.pending_refresh = false
  state.render_in_flight = true
  if not lsp.render_now(state.client_id, buf, function(render)
    local current = RENDER_STATES[buf]
    if current then
      current.render_in_flight = false
    end
    timing.log(state.root, "buffer", string.format("render_review_now rpc %.1fms buf=%s", timing.ms(start), tostring(buf)))
    if current and review_refresh_is_paused(current) then
      current.pending_refresh = true
      timing.log(current.root, "buffer", "render_review_now deferred reason=" .. review_refresh_pause_reason(current) .. " buf=" .. tostring(buf))
      schedule_pending_refresh_check(buf, current)
      return
    end
    apply_render(state.root, buf, render, state.client_id)
    current = RENDER_STATES[buf]
    if current and current.render_again then
      current.render_again = false
      if review_buffer_is_visible(buf) and not review_refresh_is_paused(current) then
        timing.log(current.root, "buffer", "render_review_now rerender buf=" .. tostring(buf))
        render_review_now(buf, current)
      else
        current.pending_refresh = true
        schedule_pending_refresh_check(buf, current)
      end
    end
  end) then
    state.render_in_flight = false
    state.pending_refresh = true
    schedule_pending_refresh_check(buf, state)
  end
end

schedule_pending_refresh_check = function(buf, state)
  if not state or state.refresh_retry_scheduled then
    return
  end

  state.refresh_retry_scheduled = true
  vim.defer_fn(function()
    local current = RENDER_STATES[buf]
    if not current then
      return
    end
    current.refresh_retry_scheduled = false
    if not current.pending_refresh then
      return
    end
    if not review_buffer_is_visible(buf) then
      return
    end
    local pause_reason = review_refresh_pause_reason(current)
    if not pause_reason then
      timing.log(current.root, "buffer", "pending_refresh flush buf=" .. tostring(buf))
      render_review_now(buf, current)
    else
      timing.log(current.root, "buffer", "pending_refresh still paused reason=" .. pause_reason .. " buf=" .. tostring(buf))
      schedule_pending_refresh_check(buf, current)
    end
  end, PAUSED_REFRESH_CHECK_MS)
end

local function request_review_refresh(buf, state)
  if not state or not vim.api.nvim_buf_is_valid(buf) then
    return
  end

  local visible = review_buffer_is_visible(buf)
  local pause_reason = review_refresh_pause_reason(state)
  if visible and not pause_reason then
    timing.log(state.root, "buffer", "request_refresh immediate buf=" .. tostring(buf))
    render_review_now(buf, state)
    return
  end

  local was_pending = state.pending_refresh
  state.pending_refresh = true
  if not was_pending then
    local reason = "not visible"
    if visible and pause_reason then
      reason = "paused " .. pause_reason
    end
    timing.log(state.root, "buffer", "request_refresh pending reason=" .. reason .. " buf=" .. tostring(buf))
  end
  if visible then
    schedule_pending_refresh_check(buf, state)
  end
end

local function mark_repo_reviews_pending(path)
  if path == "" then
    return
  end

  for buf, state in pairs(RENDER_STATES) do
    if path_is_under(state.root, path) then
      request_review_refresh(buf, state)
    end
  end
end

local function mark_repo_mirrors_pending(path)
  if path == "" then
    return
  end

  for buf, state in pairs(RENDER_STATES) do
    if path_is_under(state.root, path) then
      schedule_visible_mirror(buf)
    end
  end
end

local function flush_pending_refresh(buf)
  local state = RENDER_STATES[buf]
  if state and state.pending_refresh then
    if review_refresh_is_paused(state) then
      schedule_pending_refresh_check(buf, state)
    else
      render_review_now(buf, state)
    end
  end
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
    callback = function(args)
      schedule_visible_mirror(buf)
      sidebar.update(buf, RENDER_STATES, false, args.event)
    end,
  })
  vim.api.nvim_create_autocmd(VIEW_SAVE_EVENTS, {
    group = state.augroup,
    buffer = buf,
    callback = function()
      save_current_view(buf)
    end,
  })
  vim.api.nvim_create_autocmd("CursorMoved", {
    group = state.augroup,
    buffer = buf,
    callback = function()
      M._collapse_temporary_files_outside_cursor(buf)
      sidebar.update_preserving_focus(buf, RENDER_STATES)
    end,
  })
  vim.api.nvim_create_autocmd({ "BufEnter", "WinEnter" }, {
    group = state.augroup,
    buffer = buf,
    callback = function()
      sidebar.mark_review_active(buf, RENDER_STATES)
      restore_current_view(buf)
      flush_pending_refresh(buf)
    end,
  })
  vim.api.nvim_create_autocmd("QuitPre", {
    group = state.augroup,
    buffer = buf,
    callback = function()
      local current = RENDER_STATES[buf]
      if current then
        sidebar.close(current)
      end
    end,
  })
  vim.api.nvim_create_autocmd("BufWinLeave", {
    group = state.augroup,
    buffer = buf,
    callback = function()
      vim.schedule(function()
        if #vim.fn.win_findbuf(buf) > 0 then
          return
        end
        local current = RENDER_STATES[buf]
        if current then
          sidebar.detach(current)
        end
      end)
    end,
  })
  vim.api.nvim_create_autocmd("BufWipeout", {
    group = state.augroup,
    buffer = buf,
    callback = function()
      local current = RENDER_STATES[buf]
      if current then
        sidebar.detach(current)
      end
      RENDER_STATES[buf] = nil
    end,
  })
end

local source_change_augroup = nil

local function setup_source_change_autocmds()
  if source_change_augroup then
    return
  end

  source_change_augroup = vim.api.nvim_create_augroup(SOURCE_CHANGE_AUGROUP, { clear = true })
  vim.api.nvim_create_autocmd(SOURCE_CHANGE_EVENTS, {
    group = source_change_augroup,
    callback = function(event)
      if RENDER_STATES[event.buf] then
        return
      end
      if vim.b[event.buf][SOURCE_HELPER_BUFFER_VAR] and not vim.bo[event.buf].buflisted then
        return
      end
      mark_repo_reviews_pending(vim.api.nvim_buf_get_name(event.buf))
    end,
  })
end

local function close_composer(state)
  local review_win = state and state.composer_review_win or nil
  local return_win = state and state.composer_return_win or nil
  pcall(vim.cmd, "stopinsert")
  if state.composer_win and vim.api.nvim_win_is_valid(state.composer_win) then
    vim.api.nvim_win_close(state.composer_win, true)
  end
  if state.composer_buf and vim.api.nvim_buf_is_valid(state.composer_buf) then
    vim.api.nvim_buf_delete(state.composer_buf, { force = true })
  end
  state.composer_win = nil
  state.composer_buf = nil
  state.composer_review_win = nil
  state.composer_return_win = nil
  state.composer_return_sidebar_focus = nil
  state.composer_review_view = nil
  state.composer_allow_empty = nil
  state.composer_body_start_line = nil
  return review_win, return_win
end

local function composer_width(review_win)
  local available = vim.api.nvim_win_get_width(review_win) - COMPOSER_GUTTER_COL - 2
  return math.max(COMPOSER_MIN_WIDTH, math.min(COMPOSER_MAX_WIDTH, available))
end

local function composer_row(review_win)
  local ok, winline = pcall(vim.api.nvim_win_call, review_win, vim.fn.winline)
  if not ok then
    winline = vim.fn.winline()
  end
  if winline > COMPOSER_HEIGHT + 3 then
    return winline - COMPOSER_HEIGHT - 2
  end
  return winline
end

local function composer_config(review_win, opts)
  opts = opts or {}
  return {
    relative = "win",
    win = review_win,
    row = composer_row(review_win),
    col = COMPOSER_GUTTER_COL,
    width = composer_width(review_win),
    height = opts.height or COMPOSER_HEIGHT,
    border = "rounded",
    title = opts.title or COMPOSER_TITLE,
    footer = opts.footer or " <C-s> submit · Esc/q cancel",
    footer_pos = "right",
    style = "minimal",
  }
end

function M._configure_composer_buffer(buf)
  vim.bo[buf].textwidth = 0
  vim.bo[buf].wrapmargin = 0
  vim.bo[buf].formatoptions = (vim.bo[buf].formatoptions or ""):gsub("[tcroa]", "")
end

function M._configure_composer_window(win)
  vim.wo[win].wrap = true
  vim.wo[win].linebreak = true
  vim.wo[win].breakindent = true
  vim.wo[win].number = false
  vim.wo[win].relativenumber = false
  vim.wo[win].signcolumn = "no"
  vim.wo[win].foldcolumn = "0"
end

function M._apply_composer_highlights(buf, highlights)
  for _, highlight in ipairs(highlights or {}) do
    vim.api.nvim_buf_set_extmark(buf, NAMESPACE, highlight.line, highlight.start_col, {
      end_col = highlight.end_col,
      hl_group = highlight.group,
      priority = 1000,
    })
  end
end

local function composer_body(buf, start_line)
  local lines = vim.api.nvim_buf_get_lines(buf, start_line or 0, -1, false)
  return vim.trim(table.concat(lines, "\n"))
end

local function thread_block_range(state, thread_id)
  local first = nil
  local last = nil
  for index, row in ipairs(state.rows or {}) do
    if row.thread_id == thread_id then
      first = first or index
      last = index
    elseif first then
      break
    end
  end
  return first, last
end

local function splice_list(list, first, last, replacement)
  local next_list = {}
  for index = 1, first - 1 do
    table.insert(next_list, list[index])
  end
  for _, item in ipairs(replacement or {}) do
    table.insert(next_list, item)
  end
  for index = last + 1, #list do
    table.insert(next_list, list[index])
  end
  return next_list
end

function M._insert_list(list, after, replacement)
  local next_list = {}
  for index = 1, after do
    table.insert(next_list, list[index])
  end
  for _, item in ipairs(replacement or {}) do
    table.insert(next_list, item)
  end
  for index = after + 1, #list do
    table.insert(next_list, list[index])
  end
  return next_list
end

apply_thread_patch = function(state, review_buf, patch)
  if not state or not patch or not patch.thread_id or not patch.lines or not patch.rows then
    return
  end
  local first, last = thread_block_range(state, patch.thread_id)
  if not first or not last then
    if not patch.insert_after_line then
      return
    end
    first = patch.insert_after_line + 1
    last = patch.insert_after_line
  end

  local cursor_anchors = save_buffer_cursor_anchors(review_buf)
  local inserting = last < first
  local first_row = inserting and patch.insert_after_line or first - 1
  local last_row_exclusive = last
  vim.api.nvim_buf_clear_namespace(review_buf, NAMESPACE, first_row, last_row_exclusive)
  vim.api.nvim_buf_clear_namespace(review_buf, SOURCE_NAMESPACE, first_row, last_row_exclusive)
  vim.api.nvim_buf_clear_namespace(review_buf, M._MARKDOWN_NAMESPACE, first_row, last_row_exclusive)
  set_line_range(review_buf, first_row, last_row_exclusive, patch.lines)
  for _, highlight in ipairs(patch.highlights or {}) do
    vim.api.nvim_buf_set_extmark(review_buf, NAMESPACE, first_row + highlight.line, highlight.start_col, {
      end_col = highlight.end_col,
      hl_group = highlight.group,
    })
  end
  M._apply_comment_markdown_highlights(review_buf, patch.rows, patch.highlights, first_row)

  if inserting then
    state.lines = M._insert_list(state.lines or {}, patch.insert_after_line, patch.lines)
    state.rows = M._insert_list(state.rows or {}, patch.insert_after_line, patch.rows)
  else
    state.lines = splice_list(state.lines or {}, first, last, patch.lines)
    state.rows = splice_list(state.rows or {}, first, last, patch.rows)
  end
  for _, row in ipairs(state.rows) do
    if row.thread_id == patch.thread_id then
      row.collapsed = patch.collapsed
    end
  end
  state.sidebar = patch.sidebar or state.sidebar
  state.sidebar_counts = patch.sidebar_counts or state.sidebar_counts
  restore_buffer_cursor_anchors(review_buf, cursor_anchors, state.rows or {})
  sidebar.update_preserving_focus(review_buf, RENDER_STATES)
  timing.log(state.root, "buffer", string.format(
    "apply_thread_patch rows=%d replace=%d buf=%s",
    #(patch.rows or {}),
    last - first + 1,
    tostring(review_buf)
  ))
end

local function apply_mutation_render(state, review_buf, render)
  state.pending_refresh = false
  local review_view = state.composer_review_view
  local review_win, return_win = close_composer(state)
  if render and render.kind == "thread_patch" then
    apply_thread_patch(state, review_buf, render)
  else
    apply_render(state.root, review_buf, render, state.client_id, { force = true })
  end
  if render and review_win and vim.api.nvim_win_is_valid(review_win) and review_view then
    if render.kind == "thread_patch" then
      M._save_window_view(review_buf, review_win)
    else
      restore_win_view(review_win, review_view)
      save_current_view(review_buf)
    end
  end
  if return_win and vim.api.nvim_win_is_valid(return_win) then
    vim.api.nvim_set_current_win(return_win)
    local current = RENDER_STATES[review_buf]
    if current and current.sidebar_buf and vim.api.nvim_win_get_buf(return_win) == current.sidebar_buf then
      current.sidebar_has_focus = true
    end
  end
end

local function confirm_invalidating(input)
  if not input or not input.invalidates_later_activity then
    return true
  end
  local ok, choice = pcall(vim.fn.confirm,
    input.title .. "\n\n" .. input.message,
    COMMENT_CONFIRM_CHOICES,
    COMMENT_CONFIRM_DEFAULT,
    COMMENT_CONFIRM_DANGER
  )
  if not ok then
    return false
  end
  return choice == 1
end

local function submit_composer(review_buf, draft_buf, on_submit)
  local state = RENDER_STATES[review_buf]
  if not state then
    return
  end

  local body = composer_body(draft_buf, state.composer_body_start_line)
  if body == "" and not state.composer_allow_empty then
    vim.notify(COMMENT_EMPTY_MESSAGE, vim.log.levels.WARN)
    return
  end

  on_submit(state, body)
end

local function current_review_row(buf)
  local state = RENDER_STATES[buf]
  if not state then
    return nil
  end
  local cursor = vim.api.nvim_win_get_cursor(0)
  return state.rows[cursor[1]], cursor[1]
end

local function selected_review_range()
  local mode = vim.fn.mode()
  local start_row
  local end_row
  if mode == "v" or mode == "V" or mode == "\22" then
    start_row = vim.fn.line("v")
    end_row = vim.fn.line(".")
  else
    start_row = vim.fn.getpos("'<")[2]
    end_row = vim.fn.getpos("'>")[2]
  end
  if start_row == 0 or end_row == 0 then
    return nil, nil
  end
  if start_row > end_row then
    start_row, end_row = end_row, start_row
  end
  return start_row, end_row
end

local function visual_line_anchor(buf)
  local state = RENDER_STATES[buf]
  if not state then
    return nil
  end
  local start_row, end_row = selected_review_range()
  if not start_row then
    return nil
  end

  local path = nil
  local side = nil
  local start_line = nil
  local end_line = nil
  for index = start_row, end_row do
    local row = state.rows[index]
    if row_is_commentable(row) then
      if path and (row.path ~= path or row.side ~= side) then
        return nil
      end
      path = row.path
      side = row.side
      local row_start = row.source_start_line or row.source_line
      local row_end = row.source_line or row.source_start_line
      start_line = math.min(start_line or row_start, row_start)
      end_line = math.max(end_line or row_end, row_end)
    end
  end

  if not path or not side or not start_line or not end_line then
    return nil
  end

  return {
    scope = ROW_SCOPE_LINE,
    path = path,
    side = side,
    start_line = start_line,
    end_line = end_line,
  }
end

local function open_composer(review_buf, opts)
  local state = RENDER_STATES[review_buf]
  if not state then
    return
  end

  opts = opts or {}
  close_composer(state)
  local review_win = opts.review_win or vim.api.nvim_get_current_win()
  if not review_win or not vim.api.nvim_win_is_valid(review_win) then
    review_win = review_window_for(review_buf)
  end
  if not review_win or not vim.api.nvim_win_is_valid(review_win) then
    vim.notify(COMMENT_UNAVAILABLE_MESSAGE, vim.log.levels.WARN)
    return
  end
  local return_win = opts.return_win or review_win
  local return_sidebar_focus = state.sidebar_buf
    and return_win
    and vim.api.nvim_win_is_valid(return_win)
    and vim.api.nvim_win_get_buf(return_win) == state.sidebar_buf
  if return_sidebar_focus then
    state.sidebar_has_focus = false
  end
  local draft_buf = vim.api.nvim_create_buf(false, true)
  vim.bo[draft_buf].buftype = "nofile"
  vim.bo[draft_buf].bufhidden = "wipe"
  vim.bo[draft_buf].buflisted = false
  vim.bo[draft_buf].swapfile = false
  vim.bo[draft_buf].filetype = COMPOSER_FILETYPE
  M._configure_composer_buffer(draft_buf)
  local initial_lines = vim.split(opts.initial_body or COMPOSER_INITIAL_LINE, "\n", {
    plain = true,
  })
  local body_start_line = 0
  if opts.header_lines and #opts.header_lines > 0 then
    local lines = vim.list_extend(vim.deepcopy(opts.header_lines), initial_lines)
    body_start_line = #opts.header_lines
    initial_lines = lines
  end
  vim.api.nvim_buf_set_lines(draft_buf, 0, -1, false, initial_lines)
  M._apply_composer_highlights(draft_buf, opts.header_highlights)

  local ok, draft_win = pcall(vim.api.nvim_open_win, draft_buf, false, composer_config(review_win, opts))
  if not ok or not draft_win or not vim.api.nvim_win_is_valid(draft_win) then
    if vim.api.nvim_buf_is_valid(draft_buf) then
      vim.api.nvim_buf_delete(draft_buf, { force = true })
    end
    vim.notify(COMMENT_UNAVAILABLE_MESSAGE, vim.log.levels.WARN)
    return
  end
  M._configure_composer_window(draft_win)
  state.composer_buf = draft_buf
  state.composer_win = draft_win
  state.composer_review_win = review_win
  state.composer_return_win = return_win
  state.composer_return_sidebar_focus = return_sidebar_focus
  state.composer_review_view = save_win_view(review_win)
  state.composer_allow_empty = opts.allow_empty == true
  state.composer_body_start_line = body_start_line

  vim.keymap.set({ "n", "i" }, COMPOSER_SUBMIT_MAP, function()
    submit_composer(review_buf, draft_buf, opts.on_submit)
  end, { buffer = draft_buf, nowait = true })
  vim.keymap.set("n", COMPOSER_CANCEL_MAP, function()
    close_composer(state)
    if return_win and vim.api.nvim_win_is_valid(return_win) then
      vim.api.nvim_set_current_win(return_win)
      if return_sidebar_focus then
        state.sidebar_has_focus = true
      end
    end
    flush_pending_refresh(review_buf)
  end, { buffer = draft_buf, nowait = true })
  vim.keymap.set("n", "<Esc>", function()
    close_composer(state)
    if return_win and vim.api.nvim_win_is_valid(return_win) then
      vim.api.nvim_set_current_win(return_win)
      if return_sidebar_focus then
        state.sidebar_has_focus = true
      end
    end
    flush_pending_refresh(review_buf)
  end, { buffer = draft_buf, nowait = true })

  vim.schedule(function()
    if not vim.api.nvim_win_is_valid(draft_win) or not vim.api.nvim_buf_is_valid(draft_buf) then
      return
    end
    local entered = pcall(vim.api.nvim_set_current_win, draft_win)
    if not entered then
      close_composer(state)
      if return_win and vim.api.nvim_win_is_valid(return_win) then
        pcall(vim.api.nvim_set_current_win, return_win)
      end
      return
    end
    M._configure_composer_buffer(draft_buf)
    M._configure_composer_window(draft_win)
    pcall(vim.api.nvim_win_set_cursor, draft_win, { body_start_line + 1, 0 })
    if opts.insert_on_open then
      pcall(vim.cmd, "startinsert")
    end
  end)
end

local function create_thread_for_row(buf, row, line, composer_opts)
  local state = RENDER_STATES[buf]
  if not state then
    return
  end
  local context = thread_render_context(row)

  if row and row.kind == ROW_KIND_FILE_HEADER and row.path then
    open_composer(
      buf,
      vim.tbl_extend("force", { title = M._composer_row_title("comment", row), insert_on_open = true }, composer_opts or {}, {
        on_submit = function(state, body)
          lsp.create_thread(state.client_id, buf, {
            scope = ROW_SCOPE_FILE,
            path = row.path,
            body = body,
            context = context,
            target_line = line,
          }, function(render)
            apply_mutation_render(state, buf, render)
          end)
        end,
      })
    )
    return
  end

  if not row_is_commentable(row) then
    vim.notify(COMMENT_UNAVAILABLE_MESSAGE, vim.log.levels.WARN)
    return
  end

  open_composer(
    buf,
    vim.tbl_extend("force", { title = M._composer_row_title("comment", row), insert_on_open = true }, composer_opts or {}, {
      on_submit = function(state, body)
        lsp.create_thread(state.client_id, buf, {
          scope = row.scope or ROW_SCOPE_LINE,
          path = row.path,
          side = row.side,
          start_line = row.start_line or row.source_line,
          end_line = row.end_line or row.source_line,
          body = body,
          context = context,
          target_line = line,
        }, function(render)
          apply_mutation_render(state, buf, render)
        end)
      end,
    })
  )
end

function apply_render(root, buf, render, client_id, opts)
  if not render or not render.lines then
    return
  end

  opts = opts or {}
  local total_start = timing.now()
  local stage_start = total_start
  local existing = RENDER_STATES[buf]
  if existing and not opts.force and review_refresh_is_paused(existing) then
    existing.pending_refresh = true
    schedule_pending_refresh_check(buf, existing)
    timing.log(existing.root, "buffer", "apply_render deferred reason=" .. review_refresh_pause_reason(existing) .. " buf=" .. tostring(buf))
    return
  end
  local cursor_anchors = save_buffer_cursor_anchors(buf)
  local remembered_view = existing and existing.view or nil
  if existing then
    close_composer(existing)
  end
  local prepare_ms = timing.ms(stage_start)

  stage_start = timing.now()
  render = mask_dirty_file_diffs(root, render)
  local mask_ms = timing.ms(stage_start)

  stage_start = timing.now()
  set_lines(buf, render.lines)
  local set_lines_ms = timing.ms(stage_start)

  stage_start = timing.now()
  apply_structural_highlights(buf, render.highlights, render.rows)
  local highlights_ms = timing.ms(stage_start)

  stage_start = timing.now()
  apply_diagnostics(buf, render.diagnostics)
  local diagnostics_ms = timing.ms(stage_start)

  stage_start = timing.now()
  source_decorations.apply(root, render.source_decorations)
  local source_decorations_ms = timing.ms(stage_start)

  RENDER_STATES[buf] = {
    root = root,
    client_id = client_id,
    lines = render.lines or {},
    rows = render.rows or {},
    sidebar = render.sidebar or {},
    sidebar_counts = render.sidebar_counts or {},
    source_decorations = render.source_decorations or {},
    source_buffers = existing and existing.source_buffers or {},
    source_lsp_buffers = existing and existing.source_lsp_buffers or {},
    source_segments = existing and existing.source_segments or {},
    mirror_scheduled = false,
    mirror_batch = nil,
    mirror_again = existing and existing.mirror_again or false,
    render_in_flight = existing and existing.render_in_flight or false,
    render_again = existing and existing.render_again or false,
    pending_refresh = existing and existing.pending_refresh or false,
    refresh_retry_scheduled = existing and existing.refresh_retry_scheduled or false,
    view = remembered_view,
    sidebar_buf = existing and existing.sidebar_buf or nil,
    sidebar_win = existing and existing.sidebar_win or nil,
    sidebar_mode = existing and existing.sidebar_mode or sidebar.MODE_FILES,
    sidebar_requested = existing and existing.sidebar_requested,
    sidebar_cursor_by_mode = existing and existing.sidebar_cursor_by_mode or {},
    sidebar_augroup = existing and existing.sidebar_augroup or nil,
    sidebar_has_focus = existing and existing.sidebar_has_focus or false,
    collapsed_files = existing and existing.collapsed_files or M._load_collapsed_files(root),
    temporarily_expanded_files = existing and existing.temporarily_expanded_files or {},
  }
  if existing == nil then
    RENDER_STATES[buf].sidebar_requested = true
  end

  stage_start = timing.now()
  M._configure_review_windows(buf)
  local folds_config_ms = timing.ms(stage_start)

  stage_start = timing.now()
  restore_buffer_cursor_anchors(buf, cursor_anchors, render.rows or {})
  local restore_ms = timing.ms(stage_start)

  stage_start = timing.now()
  M._apply_file_folds(buf, RENDER_STATES[buf])
  local folds_ms = timing.ms(stage_start)

  stage_start = timing.now()
  sidebar.update_preserving_focus(buf, RENDER_STATES)
  local sidebar_ms = timing.ms(stage_start)

  stage_start = timing.now()
  setup_mirror_autocmds(buf)
  local autocmd_ms = timing.ms(stage_start)

  stage_start = timing.now()
  schedule_visible_mirror(buf)
  local mirror_ms = timing.ms(stage_start)

  timing.log(root, "buffer", string.format(
    "apply_render prepare=%.1fms mask=%.1fms lines=%.1fms highlights=%.1fms diagnostics=%.1fms source_decorations=%.1fms fold_config=%.1fms restore=%.1fms folds=%.1fms sidebar=%.1fms autocmd=%.1fms mirror_schedule=%.1fms total=%.1fms rows=%d lines=%d buf=%s",
    prepare_ms,
    mask_ms,
    set_lines_ms,
    highlights_ms,
    diagnostics_ms,
    source_decorations_ms,
    folds_config_ms,
    restore_ms,
    folds_ms,
    sidebar_ms,
    autocmd_ms,
    mirror_ms,
    timing.ms(total_start),
    #(render.rows or {}),
    #(render.lines or {}),
    tostring(buf)
  ))
end

function M.comment_current(buf, anchor)
  buf = buf or vim.api.nvim_get_current_buf()
  if not RENDER_STATES[buf] then
    return
  end

  if anchor and anchor.scope == ROW_SCOPE_FILE and anchor.path then
    open_composer(buf, {
      title = M._composer_anchor_title("comment", anchor),
      insert_on_open = true,
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
      title = M._composer_anchor_title("comment", anchor),
      insert_on_open = true,
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

  local row, line = current_review_row(buf)
  create_thread_for_row(buf, row, line)
end

function M.review_buffer_for_client(client_id)
  for buf, state in pairs(RENDER_STATES) do
    if state.client_id == client_id and vim.api.nvim_buf_is_valid(buf) then
      return buf
    end
  end
  return nil
end

function M.apply_source_decorations_for_source(buf, root)
  for _, state in pairs(RENDER_STATES) do
    if state.root == root then
      source_decorations.apply_buffer(root, buf, state.source_decorations)
      return
    end
  end
end

function M.comment_from_code_action(buf, anchor, opts)
  buf = buf or vim.api.nvim_get_current_buf()
  opts = opts or {}
  if RENDER_STATES[buf] then
    M.comment_current(buf, anchor)
    return
  end
  local review_buf = M.review_buffer_for_client(opts.client_id)
  if not review_buf then
    vim.notify(COMMENT_UNAVAILABLE_MESSAGE, vim.log.levels.WARN)
    return
  end
  M.comment_current(review_buf, anchor)
end

function M.comment_visual_selection(buf)
  buf = buf or vim.api.nvim_get_current_buf()
  local anchor = visual_line_anchor(buf)
  if not anchor then
    vim.notify(COMMENT_UNAVAILABLE_MESSAGE, vim.log.levels.WARN)
    return
  end
  M.comment_current(buf, anchor)
end

function M.reply_to_thread(buf, input, composer_opts)
  buf = buf or vim.api.nvim_get_current_buf()
  if not input or not input.thread_id then
    vim.notify(COMMENT_REPLY_UNAVAILABLE_MESSAGE, vim.log.levels.WARN)
    return
  end
  local context = M._input_thread_context(buf, input)

  open_composer(
    buf,
    vim.tbl_extend("force", { title = M._composer_reply_title(input.row), insert_on_open = true }, composer_opts or {}, {
      on_submit = function(state, body)
        lsp.reply_to_thread(state.client_id, buf, {
          thread_id = input.thread_id,
          body = body,
          context = context,
        }, function(render)
          apply_mutation_render(state, buf, render)
        end)
      end,
    })
  )
end

function M.edit_comment(buf, input)
  buf = buf or vim.api.nvim_get_current_buf()
  if not input or not input.comment_id then
    vim.notify(COMMENT_EDIT_UNAVAILABLE_MESSAGE, vim.log.levels.WARN)
    return
  end
  local context = M._input_thread_context(buf, input)

  open_composer(buf, {
    initial_body = input.body or "",
    title = M._composer_title("Edit comment"),
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
        context = context,
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
    context = M._input_thread_context(buf, input),
  }, function(render)
    apply_mutation_render(state, buf, render)
  end)
end

function M.delete_thread(buf, input)
  buf = buf or vim.api.nvim_get_current_buf()
  local state = RENDER_STATES[buf]
  if not state or not input or not input.thread_id then
    vim.notify(COMMENT_THREAD_UNAVAILABLE_MESSAGE, vim.log.levels.WARN)
    return
  end
  if
    not confirm_invalidating({
      invalidates_later_activity = true,
      title = "Delete thread?",
      message = "Deleting this thread will hide the whole thread from the visible review state.",
    })
  then
    return
  end
  lsp.delete_thread(state.client_id, buf, {
    thread_id = input.thread_id,
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
    context = M._input_thread_context(buf, input),
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
    context = M._input_thread_context(buf, input),
  }, function(render)
    apply_mutation_render(state, buf, render)
  end)
end

function M.toggle_thread_collapsed(buf, input)
  buf = buf or vim.api.nvim_get_current_buf()
  local state = RENDER_STATES[buf]
  if not state or not input or not input.thread_id then
    vim.notify(COMMENT_COLLAPSE_UNAVAILABLE_MESSAGE, vim.log.levels.WARN)
    return
  end
  local row = input.row or current_review_row(buf)
  if not row or row.thread_id ~= input.thread_id then
    vim.notify(COMMENT_COLLAPSE_UNAVAILABLE_MESSAGE, vim.log.levels.WARN)
    return
  end
  lsp.toggle_thread_collapsed(state.client_id, buf, {
    thread_id = input.thread_id,
    context = thread_render_context(row),
  }, function(render)
    apply_mutation_render(state, buf, render)
  end)
end

function M.toggle_current_file_collapsed(buf, row, line, opts)
  buf = buf or vim.api.nvim_get_current_buf()
  local state = RENDER_STATES[buf]
  if not state then
    return
  end
  if line and opts and opts.focus then
    focus_review_row(buf, line)
  end
  row = row or current_review_row(buf)
  local path = M._row_file_path(row)
  if not path then
    vim.notify(M._FILE_COLLAPSE_UNAVAILABLE_MESSAGE, vim.log.levels.WARN)
    return
  end

  state.collapsed_files = state.collapsed_files or {}
  if state.collapsed_files[path] then
    state.collapsed_files[path] = nil
  else
    state.collapsed_files[path] = true
  end
  M._save_collapsed_files(state.root, state.collapsed_files)
  M._apply_file_folds(buf, state)

  local first = M._file_block_range(state.rows, path)
  if first and state.collapsed_files[path] then
    local win = review_win_for_row(buf, first)
    if win and (not opts or opts.focus ~= false) then
      vim.api.nvim_set_current_win(win)
    end
    M._save_window_view(buf, win)
  else
    save_current_view(buf)
  end
end

function M._thread_navigation_targets(state)
  local targets = {}
  local seen = {}
  for index, row in ipairs(state.rows or {}) do
    if row.thread_id and not seen[row.thread_id] then
      seen[row.thread_id] = true
      table.insert(targets, {
        line = index,
        row = row,
        thread_id = row.thread_id,
      })
    end
  end
  return targets
end

function M._wrapped_index(index, count)
  if count == 0 then
    return nil
  end
  return ((index - 1) % count) + 1
end

function M.navigate_thread(buf, direction, opts)
  buf = buf or vim.api.nvim_get_current_buf()
  local state = RENDER_STATES[buf]
  if not state then
    return nil, nil
  end
  opts = opts or {}
  direction = direction or 1
  local targets = M._thread_navigation_targets(state)
  if #targets == 0 then
    vim.notify("No visible Peers threads", vim.log.levels.WARN)
    return nil, nil
  end

  local win = review_window_for(buf)
  local current_line = opts.current_line
  if not current_line and win and vim.api.nvim_win_is_valid(win) then
    current_line = vim.api.nvim_win_get_cursor(win)[1]
  end
  current_line = current_line or 1
  local current_row = state.rows[current_line]
  local target_index = nil
  if current_row and current_row.thread_id then
    for index, target in ipairs(targets) do
      if target.thread_id == current_row.thread_id then
        target_index = M._wrapped_index(index + direction, #targets)
        break
      end
    end
  end

  if not target_index then
    if direction >= 0 then
      for index, target in ipairs(targets) do
        if target.line > current_line then
          target_index = index
          break
        end
      end
      target_index = target_index or 1
    else
      for index = #targets, 1, -1 do
        if targets[index].line < current_line then
          target_index = index
          break
        end
      end
      target_index = target_index or #targets
    end
  end

  local target = targets[target_index]
  if target.row and target.row.path then
    M._temporarily_expand_file(buf, target.row.path)
  end
  win = review_win_for_row(buf, target.line)
  if win and opts.focus ~= false then
    vim.api.nvim_set_current_win(win)
  end
  M._save_window_view(buf, win)
  return target.line, target.row
end

function M.comment_or_reply(buf, row, line, opts)
  buf = buf or vim.api.nvim_get_current_buf()
  local state = RENDER_STATES[buf]
  if not state then
    return
  end
  opts = opts or {}
  local composer_opts = opts.composer_opts or {}
  if line then
    local review_win = opts.focus and focus_review_row(buf, line) or review_win_for_row(buf, line)
    if review_win and not composer_opts.review_win then
      composer_opts.review_win = review_win
    end
  end
  if opts.return_win and not composer_opts.return_win then
    composer_opts.return_win = opts.return_win
  end
  row = row or current_review_row(buf)
  if row and row.thread_id then
    M.reply_to_thread(buf, { thread_id = row.thread_id, row = row }, composer_opts)
    return
  end
  create_thread_for_row(buf, row, line, composer_opts)
end

function M.delete_selected_comment(buf, row, line, opts)
  buf = buf or vim.api.nvim_get_current_buf()
  local state = RENDER_STATES[buf]
  if not state then
    return
  end
  if line and opts and opts.focus then
    focus_review_row(buf, line)
  end
  row = row or current_review_row(buf)
  if not row or not row.comment_id or row.can_edit == false then
    vim.notify(COMMENT_DELETE_UNAVAILABLE_MESSAGE, vim.log.levels.WARN)
    return
  end
  M.delete_comment(buf, {
    comment_id = row.comment_id,
    invalidates_later_activity = true,
    row = row,
  })
end

function M.delete_selected_thread(buf, row, line, opts)
  buf = buf or vim.api.nvim_get_current_buf()
  local state = RENDER_STATES[buf]
  if not state then
    return
  end
  if line and opts and opts.focus then
    focus_review_row(buf, line)
  end
  row = row or current_review_row(buf)
  if not row or not row.thread_id then
    vim.notify(COMMENT_THREAD_UNAVAILABLE_MESSAGE, vim.log.levels.WARN)
    return
  end
  M.delete_thread(buf, {
    thread_id = row.thread_id,
    row = row,
  })
end

function M.toggle_selected_thread_resolved(buf, row, line, opts)
  buf = buf or vim.api.nvim_get_current_buf()
  local state = RENDER_STATES[buf]
  if not state then
    return
  end
  if line and opts and opts.focus then
    focus_review_row(buf, line)
  end
  row = row or current_review_row(buf)
  if not row or not row.thread_id then
    vim.notify(COMMENT_THREAD_UNAVAILABLE_MESSAGE, vim.log.levels.WARN)
    return
  end
  if row.resolved == true then
    M.reopen_thread(buf, { thread_id = row.thread_id, row = row })
  else
    M.resolve_thread(buf, { thread_id = row.thread_id, row = row })
  end
end

function M.agent_complete_selected_thread(buf, row, line, opts)
  buf = buf or vim.api.nvim_get_current_buf()
  local state = RENDER_STATES[buf]
  if not state then
    return
  end
  if line and opts and opts.focus then
    focus_review_row(buf, line)
  end
  row = row or current_review_row(buf)
  if not row or not row.thread_id then
    vim.notify("Peers agent thread completion is only available on comment threads", vim.log.levels.WARN)
    return
  end
  M.agent_complete_thread(buf, { thread_id = row.thread_id })
end

function M.toggle_selected_thread_collapsed(buf, row, line, opts)
  buf = buf or vim.api.nvim_get_current_buf()
  local state = RENDER_STATES[buf]
  if not state then
    return
  end
  if line and opts and opts.focus then
    focus_review_row(buf, line)
  end
  row = row or current_review_row(buf)
  if not row or not row.thread_id then
    vim.notify(COMMENT_COLLAPSE_UNAVAILABLE_MESSAGE, vim.log.levels.WARN)
    return
  end
  M.toggle_thread_collapsed(buf, { thread_id = row.thread_id, row = row })
end

function M.agent_comment_selected_thread(buf, row, line, opts)
  buf = buf or vim.api.nvim_get_current_buf()
  local state = RENDER_STATES[buf]
  if not state then
    return
  end
  if line and opts and opts.focus then
    focus_review_row(buf, line)
  end
  row = row or current_review_row(buf)
  if not row or not row.thread_id then
    vim.notify("Peers agent thread response is only available on comment threads", vim.log.levels.WARN)
    return
  end
  M.agent_comment_thread(buf, { thread_id = row.thread_id })
end

function M.ask_agent(buf, prompt)
  buf = buf or vim.api.nvim_get_current_buf()
  local state = RENDER_STATES[buf]
  if not state then
    vim.notify("Peers agent invocation is only available in a Peers review buffer", vim.log.levels.WARN)
    return
  end
  prompt = vim.trim(prompt or "")
  if prompt == "" then
    vim.notify("Peers agent prompt is empty", vim.log.levels.WARN)
    return
  end

  lsp.ask_agent(state.client_id, buf, {
    prompt = prompt,
  }, function(result)
    local suffix = result and result.thread_id and (" to " .. result.thread_id) or ""
    vim.notify("Peers agent request sent" .. suffix, vim.log.levels.INFO)
  end)
end

function M.agent_comment_thread(buf, input)
  buf = buf or vim.api.nvim_get_current_buf()
  local state = RENDER_STATES[buf]
  if not state or not input or not input.thread_id then
    vim.notify("Peers agent thread response is only available on comment threads", vim.log.levels.WARN)
    return
  end

  M.ask_agent(
    buf,
    "Please comment on Peers thread `"
      .. input.thread_id
      .. "`. Inspect it with `peers thread show "
      .. input.thread_id
      .. " --context 8`, then reply using `peers thread --agent \"Codex (GPT-5)\" reply "
      .. input.thread_id
      .. " --body ...`. Do not make code changes unless the thread explicitly asks for them."
  )
end

function M.respond_to_thread(buf, input)
  M.agent_comment_thread(buf, input)
end

function M.agent_complete_thread(buf, input)
  buf = buf or vim.api.nvim_get_current_buf()
  local state = RENDER_STATES[buf]
  if not state or not input or not input.thread_id then
    vim.notify("Peers agent thread completion is only available on comment threads", vim.log.levels.WARN)
    return
  end

  M.ask_agent(
    buf,
    "Please respond to and resolve Peers thread `"
      .. input.thread_id
      .. "`. Inspect it with `peers thread show "
      .. input.thread_id
      .. " --context 8`, make the requested code changes, then reply using `peers thread --agent \"Codex (GPT-5)\" reply "
      .. input.thread_id
      .. " --body ... --resolve` when the thread is complete. If the thread cannot be completed, reply without `--resolve` and explain the blocker."
  )
end

function M.complete_thread(buf, input)
  M.agent_complete_thread(buf, input)
end

function M.agent_review_open_threads(buf)
  buf = buf or vim.api.nvim_get_current_buf()
  local state = RENDER_STATES[buf]
  if not state then
    vim.notify("Peers agent review is only available in a Peers review buffer", vim.log.levels.WARN)
    return
  end

  M.ask_agent(
    buf,
    "Please do a full review of all currently open Peers threads in this repository. Start by running `peers thread list --status open --context 8`, inspect each thread with `peers thread show <thread-id> --context 8` when needed, make requested code changes when a thread calls for them, then reply to each completed thread using `peers thread --agent \"Codex (GPT-5)\" reply <thread-id> --body ... --resolve`. If a thread cannot be completed, reply without `--resolve` with the blocker and leave it open."
  )
end

function M._signed_delta(value)
  if value >= 0 then
    return "+" .. tostring(value)
  end
  return tostring(value)
end

function M._commit_summary(state)
  local seen_files = {}
  local seen_threads = {}
  local added = 0
  local removed = 0
  local open_threads = 0
  local closed_threads = 0

  for _, row in ipairs(state.rows or {}) do
    if row.kind == ROW_KIND_FILE_HEADER and row.path and not seen_files[row.path] then
      seen_files[row.path] = true
      added = added + (row.added_lines or 0)
      removed = removed + (row.removed_lines or 0)
    end
    if row.thread_id and seen_threads[row.thread_id] == nil then
      seen_threads[row.thread_id] = true
      if row.resolved == true then
        closed_threads = closed_threads + 1
      else
        open_threads = open_threads + 1
      end
    end
  end

  return {
    added = added,
    removed = removed,
    delta = added - removed,
    open_threads = open_threads,
    closed_threads = closed_threads,
  }
end

function M._commit_summary_line(summary)
  return M._commit_summary_header_payload(summary).lines[1]
end

function M._commit_summary_header_payload(summary, width)
  local highlights = {}
  local parts = {}
  local col = 0

  local function push(text, group)
    table.insert(parts, text)
    if group then
      table.insert(highlights, {
        line = 0,
        start_col = col,
        end_col = col + #text,
        group = group,
      })
    end
    col = col + #text
  end

  local added = "+" .. tostring(summary.added)
  local removed = "−" .. tostring(summary.removed)
  local delta = "Δ" .. M._signed_delta(summary.delta)
  local delta_group = "PeersCommitSummaryNeutral"
  if summary.delta > 0 then
    delta_group = "PeersCommitSummaryPositive"
  elseif summary.delta < 0 then
    delta_group = "PeersCommitSummaryNegative"
  end

  push("  ")
  push(added, "PeersCommitSummaryAdded")
  push(" ")
  push(removed, "PeersCommitSummaryRemoved")
  push(" ")
  push(delta, delta_group)
  push(" · ")
  push("● " .. tostring(summary.open_threads) .. " open", "PeersCommitSummaryOpen")
  push(" · ")
  push("✓ " .. tostring(summary.closed_threads) .. " closed", "PeersCommitSummaryClosed")

  local separator = string.rep("─", width or 0)
  if separator ~= "" then
    table.insert(highlights, {
      line = 1,
      start_col = 0,
      end_col = #separator,
      group = "PeersCommitSummarySeparator",
    })
  end

  return {
    lines = { table.concat(parts), separator },
    highlights = highlights,
  }
end

function M._commit_summary_header(state, review_win)
  return M._commit_summary_header_payload(M._commit_summary(state), composer_width(review_win))
end

function M._commit_agent_prompt(notes)
  notes = vim.trim(notes or "")
  local prompt =
    "Please commit the current changes in this repository. Inspect `git status` and the current diff, include only the intended working tree changes, run appropriate checks for the touched code, then create a normal git commit with a concise message. Do not amend an existing commit unless explicitly requested. If anything is ambiguous or checks fail, report the blocker instead of committing."
  if notes ~= "" then
    prompt = prompt .. "\n\nFinal notes from the user:\n" .. notes
  end
  return prompt
end

function M.agent_commit_changes(buf)
  buf = buf or vim.api.nvim_get_current_buf()
  local state = RENDER_STATES[buf]
  if not state then
    vim.notify("Peers agent commit is only available in a Peers review buffer", vim.log.levels.WARN)
    return
  end

  local review_win = review_window_for(buf)
  if not review_win then
    vim.notify("Peers agent commit is only available in a visible Peers review buffer", vim.log.levels.WARN)
    return
  end
  local return_win = vim.api.nvim_get_current_win()
  local header = M._commit_summary_header(state, review_win)
  open_composer(buf, {
    title = " Commit summary ",
    footer = " <C-s> send · Esc/q cancel",
    header_lines = header.lines,
    header_highlights = header.highlights,
    allow_empty = true,
    insert_on_open = true,
    review_win = review_win,
    return_win = return_win,
    on_submit = function(composer_state, notes)
      close_composer(composer_state)
      M.ask_agent(buf, M._commit_agent_prompt(notes))
    end,
  })
end

function M.is_review_buffer(buf)
  return RENDER_STATES[buf or vim.api.nvim_get_current_buf()] ~= nil
end

function M.remember_current_view(buf)
  save_current_view(buf or vim.api.nvim_get_current_buf())
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
      timing.log(state.root, "buffer", "refresh_from_client client=" .. tostring(client_id) .. " buf=" .. tostring(buf))
      request_review_refresh(buf, state)
    end
  end
end

function M.refresh_all()
  for buf, state in pairs(RENDER_STATES) do
    if vim.api.nvim_buf_is_valid(buf) then
      timing.log(state.root, "buffer", "refresh_all buf=" .. tostring(buf))
      request_review_refresh(buf, state)
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
    desc = "Open source file",
    nowait = true,
  })
  vim.keymap.set("n", "c", function()
    M.comment_or_reply(buf)
  end, {
    buffer = buf,
    desc = "Comment or reply in Peers review",
    nowait = true,
  })
  vim.keymap.set("n", "dd", function()
    M.delete_selected_comment(buf)
  end, {
    buffer = buf,
    desc = "Delete Peers comment",
  })
  vim.keymap.set("n", "dt", function()
    M.delete_selected_thread(buf)
  end, {
    buffer = buf,
    desc = "Delete Peers thread",
  })
  vim.keymap.set("x", "c", function()
    M.comment_visual_selection(buf)
  end, {
    buffer = buf,
    desc = "Comment on selected Peers review lines",
    nowait = true,
  })
  vim.keymap.set("n", "D", function()
    M.navigate_thread(buf, 1)
  end, {
    buffer = buf,
    desc = "Jump to next Peers thread",
    nowait = true,
  })
  vim.keymap.set("n", "U", function()
    M.navigate_thread(buf, -1)
  end, {
    buffer = buf,
    desc = "Jump to previous Peers thread",
    nowait = true,
  })
  vim.keymap.set("n", "A", function()
    M.agent_review_open_threads(buf)
  end, {
    buffer = buf,
    desc = "Ask agent to review all open Peers threads",
    nowait = true,
  })
  vim.keymap.set("n", "R", function()
    M.agent_complete_selected_thread(buf)
  end, {
    buffer = buf,
    desc = "Ask agent to respond and resolve Peers thread",
    nowait = true,
  })
  vim.keymap.set("n", "r", function()
    M.toggle_selected_thread_resolved(buf)
  end, {
    buffer = buf,
    desc = "Resolve or reopen Peers thread",
    nowait = true,
  })
  vim.keymap.set("n", "C", function()
    M.agent_comment_selected_thread(buf)
  end, {
    buffer = buf,
    desc = "Ask agent to comment on Peers thread",
    nowait = true,
  })
  vim.keymap.set("n", "x", function()
    M.toggle_selected_thread_collapsed(buf)
  end, {
    buffer = buf,
    desc = "Collapse or expand Peers thread",
    nowait = true,
  })
  vim.keymap.set("n", "X", function()
    M.toggle_current_file_collapsed(buf)
  end, {
    buffer = buf,
    desc = "Collapse or expand Peers file",
    nowait = true,
  })
  vim.keymap.set("n", "S", function()
    M.agent_commit_changes(buf)
  end, {
    buffer = buf,
    desc = "Show Peers commit summary and ask agent to commit",
    nowait = true,
  })
  sidebar.set_review_keymaps(buf, RENDER_STATES)
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
  M._configure_review_windows(buf)
  set_review_keymaps(buf)
  setup_source_change_autocmds()
  define_highlights()
  define_diff_gutter_highlights()
  set_lines(buf, {
    "Peers review " .. review_id,
    "",
    "LSP: " .. session.nvim_lsp_url,
    "",
    "Try:",
    "  vim.lsp.buf.hover()",
    "  vim.lsp.buf.code_action()",
    "  vim.lsp.buf.document_symbol()",
  })

  lsp.attach_when_ready(buf, root, session, function(client_id)
    lsp.attach_repo_sources(root, session)
    lsp.render(client_id, buf, function(render)
      apply_render(root, buf, render, client_id)
    end)
  end)
end

return M
