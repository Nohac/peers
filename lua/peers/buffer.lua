local lsp = require("peers.lsp")

local M = {}

local BUFFER_PREFIX = "peers://review/"
local FILETYPE = "peersdiff"
local NAMESPACE = vim.api.nvim_create_namespace("peers-review")
local SOURCE_NAMESPACE = vim.api.nvim_create_namespace("peers-review-source")
local ADD_FALLBACK_FG = "#3fb950"
local DELETE_FALLBACK_FG = "#f85149"
local NORMAL_FALLBACK_FG = "#f0f6fc"
local NORMAL_FALLBACK_BG = "#000000"
local HIGHLIGHT_FG = "fg"
local HIGHLIGHT_BG = "bg"
local HIGHLIGHT_BOLD = "bold"
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
local HIGHLIGHT_GROUPS = {
  PeersDiffFileHeader = { link = "Title" },
  PeersDiffHunkHeader = { link = "DiffChange" },
  PeersDiffLineNumber = { link = "LineNr" },
  PeersDiffComment = { link = "Comment" },
}
local ROW_SIDE_NEW = "new"
local ROW_KIND_ADD = "add"
local ROW_KIND_CONTEXT = "context"
local MIRROR_DEBOUNCE_MS = 30
local AUTOCMD_GROUP_PREFIX = "peers-review-source-"
local AUTOCMD_EVENTS = {
  "BufEnter",
  "WinEnter",
  "WinResized",
  "WinScrolled",
}
local CACHE_KEY_SEPARATOR = ":"
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
  vim.bo[buf].buflisted = false
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
end

local function set_lines(buf, lines)
  vim.bo[buf].readonly = false
  vim.bo[buf].modifiable = true
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
  vim.bo[buf].modifiable = false
  vim.bo[buf].readonly = true
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

local function source_buffer(root, path)
  local full_path = root .. "/" .. path
  if vim.fn.filereadable(full_path) ~= 1 then
    return nil
  end

  local buf = vim.fn.bufadd(full_path)
  vim.fn.bufload(buf)
  vim.bo[buf].buflisted = false

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

local function cache_key(row)
  return row.path .. CACHE_KEY_SEPARATOR .. tostring(row.source_line)
end

local function segments_for_row(state, row)
  local key = cache_key(row)
  local cached = state.source_segments[key]
  if cached then
    return cached
  end

  local source = source_for_row(state, row)
  if not source then
    state.source_segments[key] = {}
    return state.source_segments[key]
  end

  state.source_segments[key] = source_line_segments(source, row.source_line)
  return state.source_segments[key]
end

local function apply_line_segments(buf, review_row, code_start_col, segments)
  for _, segment in ipairs(segments) do
    vim.api.nvim_buf_set_extmark(buf, SOURCE_NAMESPACE, review_row, code_start_col + segment.start_col, {
      end_col = code_start_col + segment.end_col,
      hl_group = segment.group,
    })
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
  vim.api.nvim_create_autocmd("BufWipeout", {
    group = state.augroup,
    buffer = buf,
    callback = function()
      RENDER_STATES[buf] = nil
    end,
  })
end

local function apply_render(root, buf, render)
  if not render or not render.lines then
    return
  end

  set_lines(buf, render.lines)
  apply_structural_highlights(buf, render.highlights)
  RENDER_STATES[buf] = {
    root = root,
    rows = render.rows or {},
    source_buffers = {},
    source_segments = {},
    scheduled = false,
  }
  setup_mirror_autocmds(buf)
  mirror_visible_treesitter(buf)
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
      apply_render(root, buf, render)
    end)
  end
end

return M
