import { Link, useNavigate } from "@tanstack/react-router";
import { Check, FileText } from "lucide-react";
import { fileAnchorId, fullFileSearch } from "./fileLinks";
import type { ReviewFile } from "./reviewData";

type FileSidebarProps = {
  allFiles: boolean;
  files: ReviewFile[];
};

export function FileSidebar({ allFiles, files }: FileSidebarProps) {
  const navigate = useNavigate({ from: "/" });

  return (
    <aside className="min-h-0 border-r bg-sidebar text-sidebar-foreground">
      <div className="border-b p-3">
        <div className="text-sm font-semibold">Files</div>
        <label className="mt-3 flex items-center gap-2 text-xs text-muted-foreground">
          <input
            checked={allFiles}
            className="size-3.5 accent-primary"
            onChange={(event) => {
              void navigate({
                search: (previous) => ({
                  ...previous,
                  allFiles: event.target.checked,
                }),
              });
            }}
            type="checkbox"
          />
          Show unchanged files
        </label>
      </div>
      <div className="min-h-0 overflow-auto p-2">
        {files.map((file) => (
          <FileSidebarLink allFiles={allFiles} file={file} key={file.path} />
        ))}
      </div>
    </aside>
  );
}

type FileSidebarLinkProps = {
  allFiles: boolean;
  file: ReviewFile;
};

function FileSidebarLink({ allFiles, file }: FileSidebarLinkProps) {
  const content = (
    <>
      <FileText className="size-3.5 shrink-0 text-muted-foreground" />
      <span className="min-w-0 flex-1 truncate font-mono">{file.path}</span>
      <span className="rounded border px-1.5 py-0.5 text-[10px] uppercase text-muted-foreground">
        {file.status[0]}
      </span>
      {file.viewed ? <Check className="size-3.5 text-muted-foreground" /> : null}
      {file.commentCount > 0 ? (
        <span className="rounded-full bg-primary px-1.5 py-0.5 text-[10px] text-primary-foreground">
          {file.commentCount}
        </span>
      ) : null}
    </>
  );
  const className =
    "flex w-full items-center gap-2 rounded-md px-2 py-2 text-left text-xs no-underline hover:bg-sidebar-accent hover:text-sidebar-accent-foreground";

  if (!file.isChanged) {
    return (
      <Link className={className} search={fullFileSearch({ allFiles, path: file.path })} to="/file">
        {content}
      </Link>
    );
  }

  return (
    <Link className={className} hash={fileAnchorId(file.path)} search={{ allFiles }} to="/">
      {content}
    </Link>
  );
}
