import { expect, test } from "bun:test";
import { dispatchBody, shouldDispatch } from "./logic";

test("dispatches when nothing is active", () => {
  expect(shouldDispatch([])).toBe(true);
});

test("dispatches while one run executes and none waits", () => {
  expect(shouldDispatch([{ status: "in_progress" }])).toBe(true);
});

test("skips when one run executes and one already waits", () => {
  expect(shouldDispatch([{ status: "in_progress" }, { status: "queued" }])).toBe(false);
  expect(shouldDispatch([{ status: "in_progress" }, { status: "waiting" }])).toBe(false);
});

test("builds the dispatch body and rejects bad minutes", () => {
  expect(JSON.parse(dispatchBody(2))).toEqual({ ref: "main", inputs: { minutes: "2" } });
  expect(() => dispatchBody(0)).toThrow();
  expect(() => dispatchBody(341)).toThrow();
  expect(() => dispatchBody(1.5)).toThrow();
});
