local M = {}

local NAMESPACE = vim.api.nvim_create_namespace("peers-review-source-decorations")
local SIGN = "│"
local SIGN_PRIORITY = 5

local function normalized_path(path)
  if vim.fs and vim.fs.normalize then
    return vim.fs.normalize(path)
  end
  return path
end

local function source_buffer_repo_path(root, buf)
  if not vim.api.nvim_buf_is_valid(buf) or not vim.api.nvim_buf_is_loaded(buf) then
    return nil
  end
  if vim.bo[buf].buftype ~= "" then
    return nil
  end
  local name = vim.api.nvim_buf_get_name(buf)
  if name == "" or vim.startswith(name, "peers://") then
    return nil
  end

  local path = normalized_path(name)
  root = normalized_path(root)
  if path ~= root and not vim.startswith(path, root .. "/") then
    return nil
  end
  local relative = path:sub(#root + 2)
  if relative == "" or vim.startswith(relative, ".git/") then
    return nil
  end
  return relative
end

local function by_path(decorations)
  local grouped = {}
  for _, decoration in ipairs(decorations or {}) do
    if decoration.path and decoration.line then
      grouped[decoration.path] = grouped[decoration.path] or {}
      table.insert(grouped[decoration.path], decoration)
    end
  end
  return grouped
end

local function apply_buffer_grouped(root, buf, grouped)
  local path = source_buffer_repo_path(root, buf)
  if not path then
    return
  end

  vim.api.nvim_buf_clear_namespace(buf, NAMESPACE, 0, -1)
  for _, decoration in ipairs(grouped[path] or {}) do
    local line = decoration.line - 1
    if line >= 0 and line < vim.api.nvim_buf_line_count(buf) then
      vim.api.nvim_buf_set_extmark(buf, NAMESPACE, line, 0, {
        sign_text = SIGN,
        sign_hl_group = decoration.group,
        priority = SIGN_PRIORITY,
      })
    end
  end
end

function M.apply_buffer(root, buf, decorations)
  apply_buffer_grouped(root, buf, by_path(decorations))
end

function M.apply(root, decorations)
  local grouped = by_path(decorations)
  for _, buf in ipairs(vim.api.nvim_list_bufs()) do
    apply_buffer_grouped(root, buf, grouped)
  end
end

return M
