import { createFileRoute } from "@tanstack/react-router";
import { ReviewWorkspace } from "../features/review/ReviewWorkspace";
import { reviewSearchSchema } from "../features/review/reviewSearch";

export const Route = createFileRoute("/")({
  validateSearch: (search) => reviewSearchSchema.parse(search),
  component: ReviewRoute,
});

function ReviewRoute() {
  const search = Route.useSearch();

  return <ReviewWorkspace activeCommentId={search.comment} allFiles={search.allFiles} />;
}
