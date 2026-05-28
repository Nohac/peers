export function fileAnchorId(path: string) {
  return `file-${path.replace(/[^a-zA-Z0-9_-]/g, "-")}`;
}

type FullFileSearchInput = {
  allFiles: boolean;
  path: string;
  comment?: string;
};

export function fullFileSearch({ allFiles, comment, path }: FullFileSearchInput) {
  return comment ? { allFiles, path, comment } : { allFiles, path };
}
