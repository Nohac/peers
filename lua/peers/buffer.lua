local lsp = require("peers.lsp")

local M = {}

local BUFFER_PREFIX = "peers://review/"
local FILETYPE = "peersdiff"
local NAMESPACE = vim.api.nvim_create_namespace("peers-review")
local SOURCE_NAMESPACE = vim.api.nvim_create_namespace("peers-review-source")
local HIGHLIGHT_GROUPS = {
  PeersDiffFileHeader = { link = "Title" },
  PeersDiffHunkHeader = { link = "DiffChange" },
  PeersDiffAddGutter = { link = "DiffAdd" },
  PeersDiffDeleteGutter = { link = "DiffDelete" },
  PeersDiffLineNumber = { link = "LineNr" },
  PeersDiffComment = { link = "Comment" },
}
local ROW_SIDE_NEW = "new"
local ROW_KIND_ADD = "add"
local ROW_KIND_CONTEXT = "context"

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

local function ensure_parser(buf)
  if vim.bo[buf].filetype == "" then
    return false
  end

  local ok = pcall(vim.treesitter.get_parser, buf, vim.bo[buf].filetype)
  return ok
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

local function mirror_line_treesitter(review_buf, review_row, source_buf, source_line, code_start_col)
  local source_row = source_line - 1
  local source_text = vim.api.nvim_buf_get_lines(source_buf, source_row, source_row + 1, false)[1]
  if not source_text or source_text == "" then
    return
  end

  local active_group = nil
  local active_start = nil
  local byte_len = #source_text

  for col = 0, byte_len do
    local group = col < byte_len and capture_group_at(source_buf, source_row, col) or nil
    if group ~= active_group then
      if active_group and active_start and active_start < col then
        vim.api.nvim_buf_set_extmark(review_buf, SOURCE_NAMESPACE, review_row, code_start_col + active_start, {
          end_col = code_start_col + col,
          hl_group = active_group,
        })
      end
      active_group = group
      active_start = col
    end
  end
end

local function mirror_treesitter(root, buf, rows)
  vim.api.nvim_buf_clear_namespace(buf, SOURCE_NAMESPACE, 0, -1)
  local source_buffers = {}

  for review_row, row in ipairs(rows or {}) do
    if row.side == ROW_SIDE_NEW and (row.kind == ROW_KIND_ADD or row.kind == ROW_KIND_CONTEXT) and row.path and row.source_line then
      local source = source_buffers[row.path]
      if source == nil then
        source = source_buffer(root, row.path)
        if source and not ensure_parser(source) then
          source = false
        end
        source_buffers[row.path] = source or false
      end

      if source then
        mirror_line_treesitter(buf, review_row - 1, source, row.source_line, row.code_start_col or 0)
      end
    end
  end
end

local function apply_render(root, buf, render)
  if not render or not render.lines then
    return
  end

  set_lines(buf, render.lines)
  apply_structural_highlights(buf, render.highlights)
  mirror_treesitter(root, buf, render.rows)
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
