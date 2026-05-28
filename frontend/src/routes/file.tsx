import { createFileRoute } from "@tanstack/react-router";
import { FullFileRouteView } from "../features/review/FullFileRouteView";
import { fileReviewSearchSchema } from "../features/review/reviewSearch";

export const Route = createFileRoute("/file")({
  validateSearch: (search) => fileReviewSearchSchema.parse(search),
  component: FileRoute,
});

function FileRoute() {
  const search = Route.useSearch();

  return (
    <FullFileRouteView
      activeCommentId={search.comment}
      allFiles={search.allFiles}
      path={search.path}
    />
  );
}
