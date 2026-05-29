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
    token: optionalSearchString,
    vox: optionalSearchString,
  })
  .catch({ allFiles: false, token: undefined, vox: undefined });

export const reviewSearchSchema = z
  .object({
    allFiles: allFilesSearchParam,
    comment: optionalSearchString,
    token: optionalSearchString,
    vox: optionalSearchString,
  })
  .catch({ allFiles: false, comment: undefined, token: undefined, vox: undefined });

export const fileReviewSearchSchema = z
  .object({
    allFiles: allFilesSearchParam,
    comment: optionalSearchString,
    path: z.preprocess((value) => (typeof value === "string" ? value : ""), z.string()),
    token: optionalSearchString,
    vox: optionalSearchString,
  })
  .catch({ allFiles: false, comment: undefined, path: "", token: undefined, vox: undefined });
