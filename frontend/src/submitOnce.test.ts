import { expect, it, vi } from "vitest";
import { createSubmitOnce } from "./submitOnce";

it("runs only once while a submit is pending", async () => {
  let resolve!: (result: boolean) => void;
  const run = vi.fn(
    () =>
      new Promise<boolean>((done) => {
        resolve = done;
      }),
  );
  const submit = createSubmitOnce();

  const first = submit(run);
  const second = submit(run);

  expect(run).toHaveBeenCalledOnce();
  resolve(false);
  await expect(first).resolves.toBe(false);
  await expect(second).resolves.toBe(false);
});

it("allows retry after a failed create", async () => {
  const run = vi.fn().mockResolvedValue(false);
  const retry = vi.fn().mockResolvedValue(true);
  const submit = createSubmitOnce();

  await expect(submit(run)).resolves.toBe(false);
  await expect(submit(retry)).resolves.toBe(true);
  expect(run).toHaveBeenCalledOnce();
  expect(retry).toHaveBeenCalledOnce();
});

it("does not run again after a successful create", async () => {
  const run = vi.fn().mockResolvedValue(true);
  const submit = createSubmitOnce();

  await expect(submit(run)).resolves.toBe(true);
  await expect(submit(run)).resolves.toBe(true);
  expect(run).toHaveBeenCalledOnce();
});

it("allows retry after the create rejects", async () => {
  const error = new Error("request failed");
  const run = vi.fn().mockRejectedValueOnce(error).mockResolvedValueOnce(true);
  const submit = createSubmitOnce();

  await expect(submit(run)).rejects.toBe(error);
  await expect(submit(run)).resolves.toBe(true);
  expect(run).toHaveBeenCalledTimes(2);
});

it("allows retry after the create throws synchronously", async () => {
  const error = new Error("request failed");
  const run = vi
    .fn<() => Promise<boolean>>()
    .mockImplementationOnce(() => {
      throw error;
    })
    .mockResolvedValueOnce(true);
  const submit = createSubmitOnce();

  await expect(submit(run)).rejects.toBe(error);
  await expect(submit(run)).resolves.toBe(true);
  expect(run).toHaveBeenCalledTimes(2);
});
