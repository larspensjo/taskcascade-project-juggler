import { afterEach, expect, it, vi } from "vitest";
import { ApiError, api, reportClose, settle } from "./api";

afterEach(() => vi.unstubAllGlobals());
it("settles immediately when idle and waits for requests added while settling", async () => {
  await settle();
  const releases: ((response: Response) => void)[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(() => new Promise<Response>((resolve) => releases.push(resolve))),
  );
  const first = api.search("a");
  let idle = false;
  const settling = settle().then(() => {
    idle = true;
  });
  const second = api.search("b");
  releases[0](new Response("[]"));
  await first;
  expect(idle).toBe(false);
  releases[1](new Response("[]"));
  await second;
  await settling;
  expect(idle).toBe(true);
});
it("settles even if a request rejects", async () => {
  vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("offline")));
  const request = api.search("a").catch(() => {});
  await settle();
  await request;
});
it.each([400, 500])(
  "throws ApiError with status %i and the server message",
  async (status) => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        new Response(JSON.stringify({ message: "Title is required." }), {
          status,
        }),
      ),
    );
    const result = api.updateTask("a", {
      title: "",
      projectId: "p",
      description: "",
      scratchpad: "",
    });
    await expect(result).rejects.toBeInstanceOf(ApiError);
    await expect(result).rejects.toMatchObject({
      status,
      message: "Title is required.",
    });
    await settle();
  },
);
it("posts the close outcome to the desktop route", async () => {
  const fetch = vi.fn().mockResolvedValue(new Response(null, { status: 204 }));
  vi.stubGlobal("fetch", fetch);
  await reportClose("discarded");
  expect(fetch).toHaveBeenCalledWith(
    "/desktop/close-ready?outcome=discarded",
    expect.objectContaining({ method: "POST" }),
  );
});
