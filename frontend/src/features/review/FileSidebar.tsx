import { useEffect, useMemo, useRef, useState } from "react";
import { Link, useLocation, useNavigate, useSearch } from "@tanstack/react-router";
import { ChevronRight, FileText, FolderClosed } from "lucide-react";
import { Button } from "#/components/ui/button.tsx";
import { Checkbox } from "#/components/ui/checkbox.tsx";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "#/components/ui/collapsible.tsx";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "#/components/ui/tooltip.tsx";
import { cn } from "#/lib/utils.ts";
import { fileAnchorId, fullFileSearch } from "./fileLinks";
import { useReviewCommentActions, type ReviewFile } from "./reviewData";

type FileSidebarProps = {
  allFiles: boolean;
  files: ReviewFile[];
};

export function FileSidebar({ allFiles, files }: FileSidebarProps) {
  const navigate = useNavigate();
  const { markFileViewed } = useReviewCommentActions();
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const groups = useMemo(() => groupFiles(files), [files]);
  const activePath = useActiveFilePath(files);
  const activeGroupId = activePath ? parentPath(activePath) : undefined;
  const [collapsedGroups, setCollapsedGroups] = useState(() => new Set<string>());

  useEffect(() => {
    if (!activeGroupId) {
      return;
    }

    setCollapsedGroups((current) => {
      if (!current.has(activeGroupId)) {
        return current;
      }

      const next = new Set(current);
      next.delete(activeGroupId);
      return next;
    });
  }, [activeGroupId]);

  useEffect(() => {
    const row = scrollContainerRef.current?.querySelector("[data-active-file='true']");
    row?.scrollIntoView({ block: "nearest" });
  }, [activePath, collapsedGroups]);

  return (
    <TooltipProvider>
      <aside className="min-h-0 border-r bg-sidebar text-sidebar-foreground">
        <div className="border-b p-3">
          <div className="text-sm font-semibold">Files</div>
          <label className="mt-3 flex items-center gap-2 text-xs text-muted-foreground">
            <Checkbox
              checked={allFiles}
              onCheckedChange={(checked) => {
                navigate({
                  replace: true,
                  to: ".",
                  search: (previous) => ({
                    ...previous,
                    allFiles: checked === true,
                  }),
                });
              }}
            />
            Show unchanged files
          </label>
        </div>
        <div className="min-h-0 overflow-auto p-2" ref={scrollContainerRef}>
          {groups.map((group) => {
            const collapsed = collapsedGroups.has(group.id);

            return (
              <FileSidebarGroup
                activePath={activePath}
                allFiles={allFiles}
                collapsed={collapsed}
                group={group}
                key={group.id}
                onMarkFileViewed={markFileViewed}
                onToggle={() =>
                  setCollapsedGroups((current) => {
                    const next = new Set(current);
                    if (next.has(group.id)) {
                      next.delete(group.id);
                    } else {
                      next.add(group.id);
                    }
                    return next;
                  })
                }
              />
            );
          })}
        </div>
      </aside>
    </TooltipProvider>
  );
}

type FileSidebarGroupProps = {
  activePath?: string;
  allFiles: boolean;
  collapsed: boolean;
  group: FileGroup;
  onMarkFileViewed: (path: string, viewed: boolean) => void;
  onToggle: () => void;
};

function FileSidebarGroup({
  activePath,
  allFiles,
  collapsed,
  group,
  onMarkFileViewed,
  onToggle,
}: FileSidebarGroupProps) {
  return (
    <Collapsible onOpenChange={onToggle} open={!collapsed}>
      <Tooltip>
        <TooltipTrigger asChild>
          <CollapsibleTrigger asChild>
            <Button
              aria-label={`${collapsed ? "Expand" : "Collapse"} ${group.label}`}
              className="group h-7 w-full justify-start gap-1 px-0 pr-1 text-muted-foreground transition-none hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
              size="sm"
              variant="ghost"
            >
              <ChevronRight className="size-3.5 transition-transform group-data-[state=open]:rotate-90" />
              <FolderClosed className="size-3.5" />
              <span className="min-w-0 flex-1 truncate text-left font-mono text-[11px] font-medium [direction:rtl]">
                {group.label}
              </span>
            </Button>
          </CollapsibleTrigger>
        </TooltipTrigger>
        <TooltipContent align="start" side="right">
          <span className="font-mono">{group.label}</span>
        </TooltipContent>
      </Tooltip>
      <CollapsibleContent className="ml-5 space-y-0.5">
        {group.files.map((file) => (
          <FileSidebarLink
            active={file.path === activePath}
            allFiles={allFiles}
            file={file}
            key={file.path}
            onMarkViewed={onMarkFileViewed}
          />
        ))}
      </CollapsibleContent>
    </Collapsible>
  );
}

type FileSidebarLinkProps = {
  active: boolean;
  allFiles: boolean;
  file: ReviewFile;
  onMarkViewed: (path: string, viewed: boolean) => void;
};

function FileSidebarLink({ active, allFiles, file, onMarkViewed }: FileSidebarLinkProps) {
  const content = (
    <>
      <FileText
        className={cn(
          "size-3.5 shrink-0",
          active ? "text-sidebar-accent-foreground" : "text-muted-foreground",
        )}
      />
      <span className="min-w-0 flex-1 truncate font-mono">{basename(file.path)}</span>
      <span className="rounded border px-1.5 py-0.5 text-[10px] uppercase text-muted-foreground">
        {file.status[0]}
      </span>
      {file.commentCount > 0 ? (
        <span className="min-w-4 shrink-0 rounded-full bg-primary px-1.5 py-0.5 text-center text-[10px] text-primary-foreground">
          {file.commentCount}
        </span>
      ) : null}
    </>
  );
  const rowClassName = cn(
    "flex h-8 w-full items-center gap-1 rounded-md px-1 text-xs hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
    active && "bg-sidebar-accent text-sidebar-accent-foreground font-medium",
  );
  const linkClassName = "flex min-w-0 flex-1 items-center gap-2 py-1.5 pl-1 text-left no-underline";

  return (
    <div className={rowClassName} data-active-file={active}>
      <Tooltip>
        <TooltipTrigger asChild>
          {file.isChanged ? (
            <Link
              aria-current={active ? "location" : undefined}
              className={linkClassName}
              hash={fileAnchorId(file.path)}
              search={(previous) => ({ ...previous, allFiles })}
              to="/"
            >
              {content}
            </Link>
          ) : (
            <Link
              aria-current={active ? "location" : undefined}
              className={linkClassName}
              search={(previous) => ({
                ...previous,
                ...fullFileSearch({ allFiles, path: file.path }),
              })}
              to="/file"
            >
              {content}
            </Link>
          )}
        </TooltipTrigger>
        <TooltipContent align="start" side="right">
          <span className="font-mono">{file.path}</span>
        </TooltipContent>
      </Tooltip>
      <Tooltip>
        <TooltipTrigger asChild>
          <Checkbox
            aria-label={`${file.viewed ? "Mark not viewed" : "Mark viewed"}: ${file.path}`}
            checked={file.viewed}
            className="size-4 border-muted-foreground/50"
            onCheckedChange={(checked) => onMarkViewed(file.path, checked === true)}
          />
        </TooltipTrigger>
        <TooltipContent side="right">
          {file.viewed ? "Mark not viewed" : "Mark viewed"}
        </TooltipContent>
      </Tooltip>
    </div>
  );
}

type FileGroup = {
  id: string;
  label: string;
  files: ReviewFile[];
};

function groupFiles(files: ReviewFile[]) {
  const groups = new Map<string, FileGroup>();

  for (const file of files) {
    const id = parentPath(file.path);
    const existingGroup = groups.get(id);

    if (existingGroup) {
      existingGroup.files.push(file);
    } else {
      groups.set(id, {
        id,
        label: id,
        files: [file],
      });
    }
  }

  return [...groups.values()];
}

function useActiveFilePath(files: ReviewFile[]) {
  const location = useLocation();
  const search = useSearch({ strict: false }) as { path?: string };

  if (location.pathname === "/file" && search.path) {
    return search.path;
  }

  const hash = normalizedHash(location.hash);
  const hashFile = files.find((file) => fileAnchorId(file.path) === hash);

  if (hashFile) {
    return hashFile.path;
  }

  if (location.pathname === "/") {
    return files.find((file) => file.isChanged)?.path ?? files[0]?.path;
  }

  return undefined;
}

function normalizedHash(hash: string) {
  return hash.startsWith("#") ? hash.slice(1) : hash;
}

function parentPath(path: string) {
  const separator = path.lastIndexOf("/");
  return separator === -1 ? "/" : path.slice(0, separator);
}

function basename(path: string) {
  const separator = path.lastIndexOf("/");
  return separator === -1 ? path : path.slice(separator + 1);
}
