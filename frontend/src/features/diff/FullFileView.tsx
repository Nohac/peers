import { fullFileLinesForPath, type CommentThread, type ReviewFile } from "../review/reviewData";
import { FileLineView, type FileLine } from "./FileLineView";

type FullFileViewProps = {
  file: ReviewFile;
  threads: CommentThread[];
  lines?: FileLine[];
  activeCommentId?: string;
};

export function FullFileView({ file, threads, lines, activeCommentId }: FullFileViewProps) {
  return (
    <FileLineView
      activeCommentId={activeCommentId}
      filePath={file.path}
      lines={lines ?? fullFileLinesForPath(file.path)}
      threads={threads}
    />
  );
}
