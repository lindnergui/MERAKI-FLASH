import test from "node:test";
import assert from "node:assert/strict";
import { isNewerRelease, latestUpdate } from "../src/lib/updates.ts";

test("compara SemVer numericamente e ignora tags inválidas ou anteriores", () => {
  for (const [tag, current, expected] of [
    ["v1.0.0", "0.1.0", true], ["v1.0.0", "1.0.0", false],
    ["v1.10.0", "1.9.0", true], ["v1.9.0", "1.10.0", false],
    ["v1.0.1", "1.0.0", true], ["v1.0.0-beta.1", "1.0.0", false],
    ["v1.0.0", "1.0.0-beta.1", true], ["v01.0.0", "1.0.0", false],
    ["v1.0.0", "1.0.0+build.2", false], ["invalid", "1.0.0", false],
  ] as const) assert.equal(isNewerRelease(tag, current), expected, `${tag} / ${current}`);
});

test("só avisa sobre releases estáveis publicadas; falhas HTTP são silenciosas", async () => {
  const original = globalThis.fetch;
  try {
    for (const [release, expected] of [
      [{tag_name: "v1.1.0", draft: false, prerelease: false}, "v1.1.0"],
      [{tag_name: "v1.0.0", draft: false, prerelease: false}, null],
      [{tag_name: "v1.1.0", draft: true, prerelease: false}, null],
      [{tag_name: "v1.1.0", draft: false, prerelease: true}, null],
      [{tag_name: null}, null],
    ] as const) {
      globalThis.fetch = async () => new Response(JSON.stringify(release));
      assert.equal(await latestUpdate("1.0.0", new AbortController().signal), expected);
    }
    globalThis.fetch = async () => new Response("", { status: 403 });
    assert.equal(await latestUpdate("1.0.0", new AbortController().signal), null);
  } finally { globalThis.fetch = original; }
});
