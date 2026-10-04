import { createServer } from "node:http";
import { readFile, stat } from "node:fs/promises";
import path from "node:path";

const contentTypes = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".ico": "image/x-icon",
  ".webp": "image/webp",
  ".xml": "application/xml; charset=utf-8",
  ".txt": "text/plain; charset=utf-8",
};

export function createSiteServer(directory) {
  const root = path.resolve(directory);
  return createServer(async (request, response) => {
    response.setHeader("X-Content-Type-Options", "nosniff");
    response.setHeader("Cache-Control", "no-store");
    if (!["GET", "HEAD"].includes(request.method)) {
      response.writeHead(405, { Allow: "GET, HEAD" });
      response.end();
      return;
    }
    let url;
    let pathname;
    try {
      url = new URL(request.url, "http://127.0.0.1");
      pathname = decodeURIComponent(url.pathname);
    } catch {
      response.writeHead(400);
      response.end("Bad request");
      return;
    }
    const target = path.resolve(root, `.${pathname}`);
    if (
      pathname.includes("\0") ||
      pathname.includes("\\") ||
      (target !== root && !target.startsWith(`${root}${path.sep}`))
    ) {
      response.writeHead(400);
      response.end("Bad request");
      return;
    }
    let file = target;
    let status = 200;
    try {
      if ((await stat(file)).isDirectory()) {
        if (!url.pathname.endsWith("/")) {
          response.writeHead(301, {
            Location: `${url.pathname}/${url.search}`,
          });
          response.end();
          return;
        }
        file = path.join(file, "index.html");
      }
      await stat(file);
    } catch {
      file = path.join(root, "404.html");
      status = 404;
      response.setHeader("X-Robots-Tag", "noindex");
    }
    try {
      const body = await readFile(file);
      response.writeHead(status, {
        "Content-Type":
          contentTypes[path.extname(file)] ?? "application/octet-stream",
        "Content-Length": body.length,
      });
      response.end(request.method === "HEAD" ? undefined : body);
    } catch (error) {
      console.error("Не удалось прочитать страницу:", error.code);
      response.writeHead(500);
      response.end("Server error");
    }
  });
}

export function listen(server, port = 0) {
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(port, "127.0.0.1", () => {
      server.removeListener("error", reject);
      resolve(`http://127.0.0.1:${server.address().port}`);
    });
  });
}
