import { Protos, QuestionType } from "./tinydecide.js";
/** One stored correction: the answer's qvec and z0. */
export interface Example { v: ArrayLike<number>; z: ArrayLike<number>; }
/**
 * lists[k] holds the examples whose right answer is option k (noul: 0 = false, 1 = true).
 * beta is model.meta.beta; center is the mean qvec of the messages asked so far (optional).
 */
export function makeProtos(type: QuestionType, lists: Example[][], beta: number[], center?: ArrayLike<number> | null): Protos | null;
export function lambdaFor(type: QuestionType, lists: Example[][], center: ArrayLike<number>, beta: number[]): number;
export function meanVec(list: Example[], dh: number): number[];
export function bucketK(count: number): number;
export const LAMBDAS: number[];
