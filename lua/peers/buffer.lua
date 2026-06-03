local lsp = require("peers.lsp")
local sidebar = require("peers.sidebar")
local timing = require("peers.timing")

local M = {}

local BUFFER_PREFIX = "peers://review/"
local FILETYPE = "peersdiff"
local NAMESPACE = vim.api.nvim_create_namespace("peers-review")
local SOURCE_NAMESPACE = vim.api.nvim_create_namespace("peers-review-source")
local DIAGNOSTIC_NAMESPACE = vim.api.nvim_create_namespace("peers-review-diagnostics")
local ADD_FALLBACK_FG = "#3fb950"
local DELETE_FALLBACK_FG = "#f85149"
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
local COMPOSER_TITLE = " Peers comment "
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
local MIRROR_BATCH_BUDGET_MS = 8
local MIRROR_BATCH_MIN_ROWS = 8
local SEMANTIC_RETRY_DELAYS_MS = { 80, 240, 600 }
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
local SOURCE_SEMANTIC_TICK_VAR = "peers_source_semantic_tick"
local SOURCE_SYNTAX_READY_VAR = "peers_source_syntax_ready"
local RENDER_STATES = {}
local lsp_clients_for_buffer
local MIRROR = {
  prefix = "PeersSourceMirror",
  highlight_cache = {},
  stack_cache = {},
  semantic_token_cache = {},
  semantic_token_pending = {},
  count = 0,
  style_keys = {
    "bold",
    "italic",
    "underline",
    "undercurl",
    "underdouble",
    "underdotted",
    "underdashed",
    "strikethrough",
    "nocombine",
  },
}

local function source_tree_priority()
  return (vim.hl and vim.hl.priorities and vim.hl.priorities.user or 200) + 10
end

local function source_semantic_priority()
  return source_tree_priority() + 20
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

local function source_semantic_signature(root, path, buf)
  return table.concat({
    ensure_source_signature(root, path, buf),
    vim.b[buf][SOURCE_SEMANTIC_TICK_VAR] or 0,
  }, CACHE_KEY_SEPARATOR)
end

lsp_clients_for_buffer = function(buf)
  if not vim.lsp then
    return {}
  end
  if vim.lsp.get_clients then
    return vim.lsp.get_clients({ bufnr = buf })
  end
  if vim.lsp.get_active_clients then
    return vim.lsp.get_active_clients({ bufnr = buf })
  end
  return {}
end

local function client_supports_semantic_tokens(client, buf)
  if not client or not client.supports_method then
    return false
  end
  return client:supports_method("textDocument/semanticTokens/full", buf)
    or client:supports_method("textDocument/semanticTokens/range", buf)
end

local function semantic_highlighter_has_client(buf, client_id)
  local semantic_tokens = vim.lsp and vim.lsp.semantic_tokens or nil
  local highlighters = semantic_tokens
    and semantic_tokens.__STHighlighter
    and semantic_tokens.__STHighlighter.active
  local highlighter = highlighters and highlighters[buf] or nil
  return highlighter
    and highlighter.client_state
    and highlighter.client_state[client_id] ~= nil
end

local function start_source_semantic_tokens(buf)
  local semantic_tokens = vim.lsp and vim.lsp.semantic_tokens or nil
  if not semantic_tokens then
    return false
  end

  local started_or_active = false
  for _, client in ipairs(lsp_clients_for_buffer(buf)) do
    if client_supports_semantic_tokens(client, buf) then
      if semantic_highlighter_has_client(buf, client.id) then
        started_or_active = true
      elseif semantic_tokens._start then
        local ok = pcall(semantic_tokens._start, buf, client.id, 0)
        started_or_active = started_or_active or ok
      elseif semantic_tokens.start then
        local ok = pcall(semantic_tokens.start, buf, client.id, { debounce = 0 })
        started_or_active = started_or_active or ok
      end
    end
  end

  if started_or_active and semantic_tokens.force_refresh then
    pcall(semantic_tokens.force_refresh, buf)
  end
  return started_or_active
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

local function inspected_treesitter_syntax_groups_at(buf, row, col)
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
  return groups
end

function MIRROR.push_semantic_token_groups(target, seen, token, filetype)
  if not token or not token.type or filetype == "" then
    return
  end

  push_group(target, seen, string.format("@lsp.type.%s.%s", token.type, filetype))

  local modifiers = {}
  for modifier, enabled in pairs(token.modifiers or {}) do
    if enabled then
      table.insert(modifiers, modifier)
    end
  end
  table.sort(modifiers)

  for _, modifier in ipairs(modifiers) do
    push_group(target, seen, string.format("@lsp.mod.%s.%s", modifier, filetype))
  end
  for _, modifier in ipairs(modifiers) do
    push_group(target, seen, string.format("@lsp.typemod.%s.%s.%s", token.type, modifier, filetype))
  end
end

function MIRROR.semantic_token_modifiers(mask, token_modifiers)
  local bit = require("bit")
  local modifiers = {}
  local index = 1
  while mask and mask > 0 do
    if bit.band(mask, 1) == 1 and token_modifiers[index] then
      modifiers[token_modifiers[index]] = true
    end
    mask = bit.rshift(mask, 1)
    index = index + 1
  end
  return modifiers
end

function MIRROR.byteindex(line, encoding, index)
  local ok, byteindex = pcall(vim.str_byteindex, line, encoding or "utf-16", index, false)
  if ok then
    return byteindex
  end
  return math.min(#line, index)
end

function MIRROR.utfindex(line, encoding)
  local ok, utfindex = pcall(vim.str_utfindex, line, encoding or "utf-16")
  if ok then
    return utfindex
  end
  return #line
end

function MIRROR.semantic_ranges_from_response(buf, client, response)
  local provider = client and client.server_capabilities and client.server_capabilities.semanticTokensProvider
  local legend = provider and provider.legend or nil
  local token_types = legend and legend.tokenTypes or nil
  local token_modifiers = legend and legend.tokenModifiers or nil
  local data = response and response.data or nil
  if not token_types or not token_modifiers or not data then
    return {}
  end

  local ranges = {}
  local encoding = client.offset_encoding or "utf-16"
  local lines = vim.api.nvim_buf_get_lines(buf, 0, -1, false)
  local eol_offset = vim.bo[buf].fileformat == "dos" and 2 or 1
  local line = nil
  local start_char = 0

  for index = 1, #data, 5 do
    local delta_line = data[index]
    line = line and line + delta_line or delta_line
    start_char = delta_line == 0 and start_char + data[index + 1] or data[index + 1]

    local token_type = token_types[data[index + 3] + 1]
    if token_type then
      local end_char = start_char + data[index + 2]
      local end_line = line
      local buf_line = lines[line + 1] or ""
      local start_col = MIRROR.byteindex(buf_line, encoding, start_char)
      local next_end_char = end_char - MIRROR.utfindex(buf_line, encoding) - eol_offset

      while next_end_char > 0 do
        end_char = next_end_char
        end_line = end_line + 1
        buf_line = lines[end_line + 1] or ""
        next_end_char = next_end_char - MIRROR.utfindex(buf_line, encoding) - eol_offset
      end

      table.insert(ranges, {
        line = line,
        start_col = start_col,
        end_line = end_line,
        end_col = MIRROR.byteindex(buf_line, encoding, end_char),
        type = token_type,
        modifiers = MIRROR.semantic_token_modifiers(data[index + 4], token_modifiers),
        client_id = client.id,
      })
    end
  end

  return ranges
end

function MIRROR.semantic_ranges_by_line(ranges)
  local by_line = {}
  for _, token in ipairs(ranges or {}) do
    for line = token.line, token.end_line do
      by_line[line] = by_line[line] or {}
      table.insert(by_line[line], token)
    end
  end
  return by_line
end

function MIRROR.token_contains(token, row, col)
  if not token then
    return false
  end
  if row < token.line or row > token.end_line then
    return false
  end
  if row == token.line and col < token.start_col then
    return false
  end
  if row == token.end_line and col >= token.end_col then
    return false
  end
  return true
end

function MIRROR.cached_semantic_groups_at(buf, row, col)
  local cache = MIRROR.semantic_token_cache[buf]
  if not cache or cache.changedtick ~= vim.api.nvim_buf_get_changedtick(buf) then
    return nil
  end

  local groups = {}
  local seen = {}
  local filetype = vim.bo[buf].filetype
  local client_ids = vim.tbl_keys(cache.clients or {})
  table.sort(client_ids)
  for _, client_id in ipairs(client_ids) do
    local client_cache = cache.clients[client_id] or {}
    local tokens = client_cache.by_line and client_cache.by_line[row] or client_cache
    for _, token in ipairs(tokens or {}) do
      if MIRROR.token_contains(token, row, col) then
        MIRROR.push_semantic_token_groups(groups, seen, token, filetype)
      end
    end
  end
  return groups
end

function MIRROR.semantic_groups_at(buf, row, col)
  local cached_groups = MIRROR.cached_semantic_groups_at(buf, row, col)
  if cached_groups then
    return cached_groups
  end

  local groups = {}
  local seen = {}

  if vim.lsp and vim.lsp.semantic_tokens and vim.lsp.semantic_tokens.get_at_pos then
    local ok, tokens = pcall(vim.lsp.semantic_tokens.get_at_pos, buf, row, col)
    if ok and tokens and #tokens > 0 then
      local filetype = vim.bo[buf].filetype
      table.sort(tokens, function(left, right)
        if (left.client_id or 0) ~= (right.client_id or 0) then
          return (left.client_id or 0) < (right.client_id or 0)
        end
        if (left.start_col or 0) ~= (right.start_col or 0) then
          return (left.start_col or 0) < (right.start_col or 0)
        end
        return tostring(left.type or "") < tostring(right.type or "")
      end)
      for _, token in ipairs(tokens) do
        MIRROR.push_semantic_token_groups(groups, seen, token, filetype)
      end
      return groups
    end
  end

  local inspected = inspect_source_pos(buf, row, col)
  if not inspected then
    return groups
  end

  local tokens = vim.deepcopy(inspected.semantic_tokens or {})
  table.sort(tokens, function(left, right)
    local left_opts = left.opts or left
    local right_opts = right.opts or right
    local left_priority = left_opts.priority or 0
    local right_priority = right_opts.priority or 0
    if left_priority ~= right_priority then
      return left_priority < right_priority
    end
    return tostring(left_opts.hl_group or "") < tostring(right_opts.hl_group or "")
  end)

  for _, item in ipairs(tokens) do
    push_inspected_group(groups, seen, item.opts or item)
  end
  return groups
end

function MIRROR.request_semantic_tokens(buf, on_done)
  if not vim.lsp then
    return false
  end

  local requested = false
  local changedtick = vim.api.nvim_buf_get_changedtick(buf)
  local pending = MIRROR.semantic_token_pending[buf] or {}
  MIRROR.semantic_token_pending[buf] = pending

  for _, client in ipairs(lsp_clients_for_buffer(buf)) do
    local supports_full = client:supports_method("textDocument/semanticTokens/full", buf)
    local supports_range = client:supports_method("textDocument/semanticTokens/range", buf)
    if supports_full or supports_range then
      local key = tostring(client.id) .. ":" .. tostring(changedtick)
      if not pending[key] then
        local method = supports_full and "textDocument/semanticTokens/full" or "textDocument/semanticTokens/range"
        local params = { textDocument = vim.lsp.util.make_text_document_params(buf) }
        if not supports_full then
          params.range = {
            ["start"] = { line = 0, character = 0 },
            ["end"] = { line = vim.api.nvim_buf_line_count(buf), character = 0 },
          }
        end

        pending[key] = true
        local ok = client:request(method, params, function(err, response)
          pending[key] = nil
          if err or not response or not vim.api.nvim_buf_is_valid(buf) then
            return
          end
          if vim.api.nvim_buf_get_changedtick(buf) ~= changedtick then
            return
          end

          local cache = MIRROR.semantic_token_cache[buf] or {}
          if cache.changedtick ~= changedtick then
            cache = {
              changedtick = changedtick,
              clients = {},
            }
          end
          local ranges = MIRROR.semantic_ranges_from_response(buf, client, response)
          cache.clients[client.id] = {
            ranges = ranges,
            by_line = MIRROR.semantic_ranges_by_line(ranges),
          }
          MIRROR.semantic_token_cache[buf] = cache
          vim.b[buf][SOURCE_SEMANTIC_TICK_VAR] = (vim.b[buf][SOURCE_SEMANTIC_TICK_VAR] or 0) + 1
          if on_done then
            vim.schedule(on_done)
          end
        end, buf)
        requested = requested or ok
        if not ok then
          pending[key] = nil
        end
      end
    end
  end

  return requested
end

local function highlight_groups_key(groups)
  return table.concat(groups, "\0")
end

function MIRROR.resolved_highlight_spec(groups)
  local spec = {}
  local has_style = false

  for _, group in ipairs(groups or {}) do
    local ok, highlight = pcall(vim.api.nvim_get_hl, 0, { name = group, link = false })
    if ok and highlight then
      if highlight.fg then
        spec.fg = highlight.fg
        has_style = true
      end
      if highlight.sp then
        spec.sp = highlight.sp
        has_style = true
      end
      for _, key in ipairs(MIRROR.style_keys) do
        if highlight[key] ~= nil then
          spec[key] = highlight[key]
          if highlight[key] then
            has_style = true
          end
        end
      end
    end
  end

  if not has_style then
    return nil
  end
  return spec
end

function MIRROR.highlight_spec_key(spec)
  if not spec then
    return ""
  end

  local parts = {}
  if spec.fg then
    table.insert(parts, "fg=" .. tostring(spec.fg))
  end
  if spec.sp then
    table.insert(parts, "sp=" .. tostring(spec.sp))
  end
  for _, key in ipairs(MIRROR.style_keys) do
    if spec[key] ~= nil then
      table.insert(parts, key .. "=" .. tostring(spec[key]))
    end
  end
  return table.concat(parts, ";")
end

function MIRROR.highlight_group(groups)
  local stack_key = highlight_groups_key(groups)
  if stack_key == "" then
    return nil
  end

  local stack_cached = MIRROR.stack_cache[stack_key]
  if stack_cached ~= nil then
    return stack_cached or nil
  end

  local spec = MIRROR.resolved_highlight_spec(groups)
  if not spec then
    MIRROR.stack_cache[stack_key] = false
    return nil
  end

  local key = MIRROR.highlight_spec_key(spec)
  local cached = MIRROR.highlight_cache[key]
  if cached then
    MIRROR.stack_cache[stack_key] = cached
    return cached
  end

  MIRROR.count = MIRROR.count + 1
  local group = MIRROR.prefix .. MIRROR.count
  vim.api.nvim_set_hl(0, group, spec)
  MIRROR.highlight_cache[key] = group
  MIRROR.stack_cache[stack_key] = group
  return group
end

local function source_line_segments(source_buf, source_line, groups_at)
  local source_row = source_line - 1
  local source_text = vim.api.nvim_buf_get_lines(source_buf, source_row, source_row + 1, false)[1]
  if not source_text or source_text == "" then
    return {}
  end

  local segments = {}
  local active_group = nil
  local active_key = ""
  local active_start = nil
  local byte_len = #source_text

  for col = 0, byte_len do
    local groups = col < byte_len and groups_at(source_buf, source_row, col) or {}
    local group = col < byte_len and MIRROR.highlight_group(groups) or nil
    local key = group or ""
    if key ~= active_key then
      if active_group and active_start and active_start < col then
        table.insert(segments, {
          start_col = active_start,
          end_col = col,
          group = active_group,
        })
      end
      active_group = group
      active_key = key
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

  lines[row.source_line] = source_line_segments(source, row.source_line, inspected_treesitter_syntax_groups_at)
  return lines[row.source_line]
end

local schedule_visible_mirror

local function schedule_semantic_mirror_retry(buf, state)
  if state.semantic_retry_scheduled then
    return
  end

  state.semantic_retry_scheduled = true
  local pending = #SEMANTIC_RETRY_DELAYS_MS
  for _, delay in ipairs(SEMANTIC_RETRY_DELAYS_MS) do
    vim.defer_fn(function()
      local current = RENDER_STATES[buf]
      if not current then
        return
      end
      schedule_visible_mirror(buf)
      pending = pending - 1
      if pending <= 0 then
        current.semantic_retry_scheduled = false
      end
    end, delay)
  end
end

local function request_source_semantic_refresh(buf, state, row, source, signature)
  if not vim.lsp or not vim.lsp.semantic_tokens or not vim.lsp.semantic_tokens.force_refresh then
    return
  end

  state.source_semantic_refreshes = state.source_semantic_refreshes or {}
  if state.source_semantic_refreshes[row.path] == signature then
    return
  end

  state.source_semantic_refreshes[row.path] = signature
  local function on_done()
    local current = RENDER_STATES[buf]
    if not current then
      return
    end
    current.source_semantic_segments = current.source_semantic_segments or {}
    current.source_semantic_segments[row.path] = nil
    schedule_visible_mirror(buf)
  end
  local started = start_source_semantic_tokens(source)
  local requested = MIRROR.request_semantic_tokens(source, on_done)
  if started or requested then
    timing.log(state.root, "buffer", "source semantic refresh requested path=" .. tostring(row.path))
    schedule_semantic_mirror_retry(buf, state)
  end
end

local function semantic_segments_for_row(buf, state, row)
  local source = source_for_row(state, row)
  if not source then
    return {}
  end

  local signature = source_semantic_signature(state.root, row.path, source)
  state.source_semantic_segments = state.source_semantic_segments or {}
  local file_cache = state.source_semantic_segments[row.path]
  if not file_cache or file_cache.signature ~= signature then
    file_cache = {
      signature = signature,
      lines = {},
    }
    state.source_semantic_segments[row.path] = file_cache
  end

  local cached = file_cache.lines[row.source_line]
  if cached then
    return cached
  end

  local segments = source_line_segments(source, row.source_line, MIRROR.semantic_groups_at)
  if #segments == 0 then
    request_source_semantic_refresh(buf, state, row, source, signature)
    return segments
  end

  file_cache.lines[row.source_line] = segments
  return file_cache.lines[row.source_line]
end

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
      if segment.group then
        vim.api.nvim_buf_set_extmark(buf, SOURCE_NAMESPACE, review_row, start_col, {
          end_col = end_col,
          hl_group = segment.group,
          priority = base_priority,
        })
      else
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

local function clamp_cursor(buf, line, col)
  local line_count = math.max(1, vim.api.nvim_buf_line_count(buf))
  line = math.max(1, math.min(line or 1, line_count))
  local text = vim.api.nvim_buf_get_lines(buf, line - 1, line, false)[1] or ""
  col = math.max(0, math.min(col or 0, #text))
  return line, col
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
  apply_line_segments(buf, review_row, row.code_start_col or 0, semantic_segments_for_row(buf, state, row), source_semantic_priority())
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

local function mark_source_semantics_changed(buf)
  vim.b[buf][SOURCE_SEMANTIC_TICK_VAR] = (vim.b[buf][SOURCE_SEMANTIC_TICK_VAR] or 0) + 1
end

function MIRROR.reset()
  MIRROR.highlight_cache = {}
  MIRROR.stack_cache = {}
  MIRROR.semantic_token_cache = {}
  MIRROR.semantic_token_pending = {}
  MIRROR.count = 0
  define_highlights()
  define_diff_gutter_highlights()
  for buf, state in pairs(RENDER_STATES) do
    state.source_segments = {}
    state.source_semantic_segments = {}
    if vim.api.nvim_buf_is_valid(buf) then
      schedule_visible_mirror(buf)
      sidebar.update(buf, RENDER_STATES, false, "ColorScheme")
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
  vim.api.nvim_create_autocmd("LspTokenUpdate", {
    group = source_change_augroup,
    callback = function(event)
      if RENDER_STATES[event.buf] then
        return
      end
      mark_source_semantics_changed(event.buf)
      mark_repo_mirrors_pending(vim.api.nvim_buf_get_name(event.buf))
    end,
  })
  vim.api.nvim_create_autocmd("LspAttach", {
    group = source_change_augroup,
    callback = function(event)
      if RENDER_STATES[event.buf] then
        return
      end
      if not vim.b[event.buf][SOURCE_HELPER_BUFFER_VAR] then
        return
      end
      local function on_done()
        mark_source_semantics_changed(event.buf)
        mark_repo_mirrors_pending(vim.api.nvim_buf_get_name(event.buf))
      end
      local started = start_source_semantic_tokens(event.buf)
      local requested = MIRROR.request_semantic_tokens(event.buf, on_done)
      if started or requested then
        mark_source_semantics_changed(event.buf)
        mark_repo_mirrors_pending(vim.api.nvim_buf_get_name(event.buf))
      end
    end,
  })
  vim.api.nvim_create_autocmd("ColorScheme", {
    group = source_change_augroup,
    callback = function()
      MIRROR.reset()
    end,
  })
end

local function close_composer(state)
  local review_win = state and state.composer_review_win or nil
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
  state.composer_review_view = nil
  return review_win
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
  state.pending_refresh = false
  local review_view = state.composer_review_view
  local review_win = close_composer(state)
  if
    review_win
    and vim.api.nvim_win_is_valid(review_win)
    and vim.api.nvim_win_get_buf(review_win) == review_buf
  then
    vim.api.nvim_set_current_win(review_win)
  end
  apply_render(state.root, review_buf, render, state.client_id)
  if review_win and vim.api.nvim_win_is_valid(review_win) and review_view then
    restore_win_view(review_win, review_view)
    save_current_view(review_buf)
  end
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
  state.composer_review_win = review_win
  state.composer_review_view = save_win_view(review_win)

  vim.keymap.set({ "n", "i" }, COMPOSER_SUBMIT_MAP, function()
    submit_composer(review_buf, draft_buf, opts.on_submit)
  end, { buffer = draft_buf, nowait = true })
  vim.keymap.set("n", COMPOSER_CANCEL_MAP, function()
    close_composer(state)
    flush_pending_refresh(review_buf)
  end, { buffer = draft_buf, nowait = true })
  vim.keymap.set("n", "<Esc>", function()
    close_composer(state)
    flush_pending_refresh(review_buf)
  end, { buffer = draft_buf, nowait = true })

  vim.cmd("startinsert")
end

function apply_render(root, buf, render, client_id)
  if not render or not render.lines then
    return
  end

  local total_start = timing.now()
  local stage_start = total_start
  local existing = RENDER_STATES[buf]
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
  apply_structural_highlights(buf, render.highlights)
  local highlights_ms = timing.ms(stage_start)

  stage_start = timing.now()
  apply_diagnostics(buf, render.diagnostics)
  local diagnostics_ms = timing.ms(stage_start)

  RENDER_STATES[buf] = {
    root = root,
    client_id = client_id,
    lines = render.lines or {},
    rows = render.rows or {},
    sidebar_counts = render.sidebar_counts or {},
    source_buffers = existing and existing.source_buffers or {},
    source_lsp_buffers = existing and existing.source_lsp_buffers or {},
    source_segments = existing and existing.source_segments or {},
    source_semantic_segments = existing and existing.source_semantic_segments or {},
    source_semantic_refreshes = existing and existing.source_semantic_refreshes or {},
    mirror_scheduled = false,
    mirror_batch = nil,
    mirror_again = existing and existing.mirror_again or false,
    semantic_retry_scheduled = existing and existing.semantic_retry_scheduled or false,
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
  }
  if existing == nil then
    RENDER_STATES[buf].sidebar_requested = true
  end

  stage_start = timing.now()
  restore_buffer_cursor_anchors(buf, cursor_anchors, render.rows or {})
  local restore_ms = timing.ms(stage_start)

  stage_start = timing.now()
  sidebar.update_preserving_focus(buf, RENDER_STATES)
  local sidebar_ms = timing.ms(stage_start)

  stage_start = timing.now()
  setup_mirror_autocmds(buf)
  local autocmd_ms = timing.ms(stage_start)

  stage_start = timing.now()
  vim.api.nvim_buf_clear_namespace(buf, SOURCE_NAMESPACE, 0, -1)
  schedule_visible_mirror(buf)
  local mirror_ms = timing.ms(stage_start)

  timing.log(root, "buffer", string.format(
    "apply_render prepare=%.1fms mask=%.1fms lines=%.1fms highlights=%.1fms diagnostics=%.1fms restore=%.1fms sidebar=%.1fms autocmd=%.1fms mirror_schedule=%.1fms total=%.1fms rows=%d lines=%d buf=%s",
    prepare_ms,
    mask_ms,
    set_lines_ms,
    highlights_ms,
    diagnostics_ms,
    restore_ms,
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
    desc = OPEN_SOURCE_DESC,
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
  set_review_keymaps(buf)
  setup_source_change_autocmds()
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

  lsp.attach_when_ready(buf, root, session, function(client_id)
    lsp.render(client_id, buf, function(render)
      apply_render(root, buf, render, client_id)
    end)
  end)
end

return M
