import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { describe, expect, it } from "vitest";
import { type ExportTask, formatTasksMarkdown } from "./exportMarkdown";

function task(title: string, description = "", scratchpad = ""): ExportTask {
  return {
    title,
    description,
    scratchpad,
    projectId: "e",
    completedAt: null,
    deletedAt: null,
  };
}

function format(tasks: ExportTask[]): string {
  return formatTasksMarkdown({
    view: "active",
    query: "",
    projects: [{ id: "e", name: "Engine" }],
    selectedProjectIds: ["e"],
    tasks,
  });
}

function render(text: string): string {
  return renderToStaticMarkup(
    createElement(ReactMarkdown, { remarkPlugins: [remarkGfm] }, text),
  );
}

describe("rendering the Markdown export", () => {
  it("keeps each task and its fields out of the previous task's code blocks", () => {
    const html = render(
      format([
        task(
          "Main fields",
          "# Big\n\n## Bigger\n\n```\n# Code",
          "~~~~\nScratch code",
        ),
        task("List fence", "- step\n  ```sh\n  echo done"),
        task(
          "Closed by a list boundary",
          "- step\n  ```\n  code\nAfter the list",
        ),
        task("Empty fields"),
      ]),
    );
    expect(html.match(/<h1>.*?<\/h1>/g)).toEqual([
      "<h1>TaskCascade — Active tasks</h1>",
    ]);
    expect(html.match(/<h2>.*?<\/h2>/g)).toEqual([
      "<h2>1. Main fields</h2>",
      "<h2>2. List fence</h2>",
      "<h2>3. Closed by a list boundary</h2>",
      "<h2>4. Empty fields</h2>",
    ]);
    expect(html.match(/<h3>.*?<\/h3>/g)).toEqual(
      Array.from({ length: 4 }, () => [
        "<h3>Description</h3>",
        "<h3>Scratchpad</h3>",
      ]).flat(),
    );
    expect(html).toContain("<h4>Big</h4>");
    expect(html).toContain("<h5>Bigger</h5>");
    const outsideCode = html.replace(/<pre>[\s\S]*?<\/pre>/g, "");
    expect(outsideCode).toContain("<p>After the list</p>");
    expect(outsideCode.match(/<h2>/g)).toHaveLength(4);
    expect(
      outsideCode
        .split("<h2>4. Empty fields</h2>")[1]
        .match(/<em>\(empty\)<\/em>/g),
    ).toHaveLength(2);
  });

  it("copies raw HTML verbatim and the renderer treats it as text", () => {
    const raw = "<script>alert(1)</script>\n\n<b>x</b>";
    const text = format([task("Raw HTML", raw)]);
    expect(text).toContain(raw);
    expect(render(text)).not.toMatch(/<script|<b>/);
  });
});
