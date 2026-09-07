---
name: gcsdrop
description: Publish an HTML report to a GCS bucket and get a shareable URL. Use when the user says "gcsdrop this", "share this report", "give me a link to this HTML", or asks to turn generated HTML into a URL they can paste into an issue or a chat.
---

# gcsdrop

Uploads HTML to a GCS bucket the user owns and prints back a URL. This assumes gcsdrop is
already set up (bucket created, IAM granted, credentials working) — full setup, every error
message explained, and the reasoning behind all of it is in [`../../README.md`](../../README.md).
Do not try to create a bucket or change IAM from here; if setup looks broken, tell the human and
point them at the README.

Before anything else, confirm `GCSDROP_BUCKET` is set: `echo $GCSDROP_BUCKET`. If empty, stop
and tell the human.

Credentials differ by machine — look before you run. If `GOOGLE_APPLICATION_CREDENTIALS` is
already set, that's deliberate: pass it through, never unset or override it. If not, check for a
scoped impersonation file at `~/.config/gcsdrop/adc.json` — this project's convention, not a
guarantee, so it may live elsewhere — and set the variable to it if you find one. Otherwise just
run gcsdrop: it needs nothing extra on GCE/GKE, and on a laptop its own error explains the fix —
show the human that output rather than improvising one, and **never create the credentials file
yourself**; that's a human setup step, documented in [`../../README.md`](../../README.md).

## 1. Find the binary

`gcsdrop` is very likely **not on `PATH`**. Check first:

```bash
gcsdrop --version
```

On `command not found`, either call it by absolute path (`$HOME/bin/gcsdrop ...`) or, once per
shell:

```bash
export PATH="$HOME/bin:$PATH"
```

If it isn't installed at all yet, first pick `$ASSET` for this machine — check both `uname -s`
and `uname -m`, since `arm64` alone is ambiguous (it's what `uname -m` prints on **both** Linux
arm64 and macOS): `Linux`+`x86_64` → `gcsdrop-linux-amd64`, `Linux`+`aarch64`/`arm64` →
`gcsdrop-linux-arm64`, `Darwin`+`arm64` → `gcsdrop-macos-arm64`. Then download and verify before
extracting — the checksum only catches a corrupted download, not a compromised release, but it is
still worth doing:

```bash
mkdir -p "$HOME/bin"
cd "$(mktemp -d)"

curl -fsSLO "https://github.com/botrun/gcsdrop/releases/latest/download/$ASSET.tar.gz"
curl -fsSLO "https://github.com/botrun/gcsdrop/releases/latest/download/$ASSET.tar.gz.sha256"

sha256sum -c "$ASSET.tar.gz.sha256"      # macOS: shasum -a 256 -c "$ASSET.tar.gz.sha256"
tar -xzf "$ASSET.tar.gz" -C "$HOME/bin"
chmod +x "$HOME/bin/gcsdrop"
```

The `-f` in `-fsSLO` matters: without it, a 404 response body gets saved as if it were the
tarball, and `sha256sum -c` would fail on it — but with `-f`, curl exits non-zero on 404 instead.
⚠️ No release has been published yet as of this writing, so this command may 404 — if it does,
tell the human and stop. Do not fall back to `cargo install` or building from source; the target
container has no Rust toolchain.

## 2. Generate ONE self-contained HTML file

**This is the step that breaks uploads if skipped.** gcsdrop signs exactly one GCS object. A
directory of `index.html` + `style.css` + `chart.png` gets a signed URL for the HTML page only —
the CSS and images 403, and the human gets a broken page.

So:

- Inline all CSS in a `<style>` tag — no separate `.css` file.
- Embed every image as a `data:` URI — no separate image files.
- No external CDN scripts or stylesheets — the viewer's network may block them, and each
  external asset is one more thing that can fail to load.
- Include `<meta name="viewport" content="width=device-width, initial-scale=1">`.

## 3. Upload

```bash
URL=$(gcsdrop ./report.html --expires 7d)
STATUS=$?
```

gcsdrop prints exactly one line to stdout — the URL — and puts everything else (progress,
errors) on stderr, so capturing stdout gives you the URL with nothing to strip. `$STATUS` is
gcsdrop's real exit code because nothing is piped in between: `0` means it worked, non-zero
means it didn't and the reason is already on stderr.

**Always pass `--expires 7d`.** The default is `1h`, which is wrong for this job: the link goes
to a person who has to read the report, and they may not open it until tomorrow. A link that
died overnight means they come back asking for a new one. `7d` is the maximum a V4 signed URL
allows, so it is the longest you can give them.

Use a shorter value only if the human asked for one.

## 4. Verify

The URL is a signed link on `storage.googleapis.com` that any HTTP client can fetch:

```bash
curl -sSI "$URL" | head -3
```

Expect `HTTP/2 200` and `content-type: text/html`. **Quote the variable** — a signed URL
contains `&`, which an unquoted shell splits into separate arguments (or backgrounds the
command).

If `$STATUS` is non-zero, don't guess why — gcsdrop already printed the reason to stderr; show
the human that output.

## 5. Report to the human

Give two things: the URL (`$URL`) and when it expires.

⚠️ **A signed URL is a bearer credential.** Anyone holding it can read the object until it
expires — no login, no identity check. Posting one into a public GitHub issue or public channel
publishes the content to whoever finds it. Confirm with the human before putting a signed URL
anywhere public.
