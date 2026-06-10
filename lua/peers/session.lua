local M = {}

local active_job = nil
local active_root = nil
local MIN_NVIM_VERSION = { 0, 12, 0 }
local SESSION_FILE_NAME = "session.json"
local SESSION_DECODE_WARNING = "Could not decode Peers session.json"
local STALE_SESSION_MESSAGE = "Discarded stale Peers session"
local NON_REALTIME_SESSION_MESSAGE = "Discarded Peers session without realtime support"
local NO_GIT_REPO_ERROR = "Not inside a git repo"
local START_FAILURE_ERROR = "Failed to start Peers"
local START_TIMEOUT_ERROR = "Peers session did not start"
local NVIM_VERSION_ERROR_PREFIX = "Peers.nvim requires Neovim "
local BINARY_NOT_EXECUTABLE_ERROR = "Peers binary is not executable"
local COMMAND_SESSION = "session"
local COMMAND_DIFF = "diff"
local COMMAND_REVIEW = "review"
local ARG_NVIM_LISTEN = "--nvim-listen"
local ARG_CACHED = "--cached"
local ARG_ALL = "--all"
local ARG_BASE = "--base"
local ARG_HEAD = "--head"
local DOT_PEERS_DIR = ".peers"
local NVIM_SOCKET_PREFIX = "nvim-"
local NVIM_SOCKET_SUFFIX = ".sock"
local MKDIR_PARENTS = "p"
local STOP_TIMEOUT_MS = 1000

local nvim_servername = nil

local function read_file(path)
  local file = io.open(path, "r")
  if not file then
    return nil
  end
  local body = file:read("*a")
  file:close()
  return body
end

local function session_path(root)
  return root .. "/.peers/" .. SESSION_FILE_NAME
end

local function pid_is_live(pid)
  if not pid or not vim.uv or not vim.uv.kill then
    return true
  end

  local ok = vim.uv.kill(pid, 0)
  return ok == 0 or ok == true
end

local function discard_session(root, active)
  os.remove(session_path(root))
  if active and active.pid then
    vim.notify(STALE_SESSION_MESSAGE .. " pid " .. tostring(active.pid), vim.log.levels.INFO)
  end
end

local function command_for_launch(config, root, launch)
  local command
  if type(config.binary) == "table" then
    command = vim.deepcopy(config.binary)
  else
    command = { config.binary }
  end

  if launch.mode == COMMAND_REVIEW then
    vim.list_extend(command, { COMMAND_SESSION, COMMAND_REVIEW })
    if launch.base then
      vim.list_extend(command, { ARG_BASE, launch.base })
    end
    if launch.head then
      vim.list_extend(command, { ARG_HEAD, launch.head })
    end
  else
    vim.list_extend(command, { COMMAND_SESSION, COMMAND_DIFF })
    if launch.cached then
      vim.list_extend(command, { ARG_CACHED })
    end
    if launch.all then
      vim.list_extend(command, { ARG_ALL })
    end
  end
  vim.list_extend(command, { ARG_NVIM_LISTEN, M.nvim_server(root) })
  return command
end

local function command_binary(command)
  return command and command[1]
end

local function executable_command(command)
  local binary = command_binary(command)
  if not binary or vim.fn.executable(binary) == 1 then
    return command
  end

  error(BINARY_NOT_EXECUTABLE_ERROR .. ": " .. tostring(binary))
end

local function check_nvim_version()
  if vim.fn.has("nvim-0.12") == 0 then
    local required = table.concat(MIN_NVIM_VERSION, ".")
    error(NVIM_VERSION_ERROR_PREFIX .. required .. " or newer")
  end
end

function M.repo_root()
  check_nvim_version()
  local git = vim.fs.find(".git", { upward = true })[1]
  if not git then
    error(NO_GIT_REPO_ERROR)
  end
  return vim.fs.dirname(git)
end

function M.repo_review_id(root)
  local _ = root
  return "repo"
end

function M.nvim_server(root)
  if nvim_servername then
    return nvim_servername
  end

  local socket_dir = root .. "/" .. DOT_PEERS_DIR
  vim.fn.mkdir(socket_dir, MKDIR_PARENTS)
  local socket = socket_dir .. "/" .. NVIM_SOCKET_PREFIX .. tostring(vim.uv.os_getpid()) .. NVIM_SOCKET_SUFFIX
  os.remove(socket)
  nvim_servername = vim.fn.serverstart(socket)
  return nvim_servername
end

function M.read_session(root, _review_id)
  local body = read_file(session_path(root))
  if not body then
    return nil
  end

  local ok, decoded = pcall(vim.json.decode, body)
  if not ok then
    discard_session(root)
    vim.notify(SESSION_DECODE_WARNING, vim.log.levels.WARN)
    return nil
  end
  return decoded
end

function M.read_live_session(root, review_id)
  local active = M.read_session(root, review_id)
  if active and active.realtime ~= true then
    discard_session(root, active)
    vim.notify(NON_REALTIME_SESSION_MESSAGE, vim.log.levels.INFO)
    return nil
  end
  if active and pid_is_live(active.pid) then
    return active
  end
  if active then
    discard_session(root, active)
  end
  return nil
end

function M.start(config, root, launch)
  if active_job then
    M.stop()
  end
  active_job = vim.fn.jobstart(executable_command(command_for_launch(config, root, launch or {})), {
    cwd = root,
    stdout_buffered = false,
    stderr_buffered = false,
    on_exit = function()
      active_job = nil
      active_root = nil
    end,
  })

  if active_job <= 0 then
    active_job = nil
    active_root = nil
    error(START_FAILURE_ERROR)
  end
  active_root = root
end

function M.started_by_nvim()
  return active_job ~= nil
end

function M.wait_for_session(config, root, review_id, on_ready)
  local remaining = math.max(1, math.floor(config.start_timeout_ms / config.poll_interval_ms))

  local function poll()
    local active = M.read_live_session(root, review_id)
    if active then
      on_ready(active)
      return
    end

    remaining = remaining - 1
    if remaining <= 0 then
      vim.notify(START_TIMEOUT_ERROR, vim.log.levels.ERROR)
      return
    end

    vim.defer_fn(poll, config.poll_interval_ms)
  end

  poll()
end

function M.wait_for_current_session(config, root, on_ready)
  local remaining = math.max(1, math.floor(config.start_timeout_ms / config.poll_interval_ms))

  local function poll()
    local review_id = "repo"
    local active = M.read_live_session(root, review_id)
    if active then
      on_ready(review_id, active)
      return
    end

    remaining = remaining - 1
    if remaining <= 0 then
      vim.notify(START_TIMEOUT_ERROR, vim.log.levels.ERROR)
      return
    end

    vim.defer_fn(poll, config.poll_interval_ms)
  end

  poll()
end

function M.stop()
  if active_job then
    local job = active_job
    vim.fn.jobstop(job)
    vim.fn.jobwait({ job }, STOP_TIMEOUT_MS)
    active_job = nil
    active_root = nil
  end
end

return M
