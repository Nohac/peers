import { z } from "zod";

const optionalSearchString = z.preprocess(
  (value) => (typeof value === "string" && value.length > 0 ? value : undefined),
  z.string().optional(),
);

const allFilesSearchParam = z.preprocess(
  (value) => value === true || value === "true" || value === "1",
  z.boolean(),
);

export const rootReviewSearchSchema = z
  .object({
    allFiles: allFilesSearchParam,
  })
  .catch({ allFiles: false });

export const reviewSearchSchema = z
  .object({
    allFiles: allFilesSearchParam,
    comment: optionalSearchString,
  })
  .catch({ allFiles: false, comment: undefined });

export const fileReviewSearchSchema = z
  .object({
    allFiles: allFilesSearchParam,
    comment: optionalSearchString,
    path: z.preprocess((value) => (typeof value === "string" ? value : ""), z.string()),
  })
  .catch({ allFiles: false, comment: undefined, path: "" });
