# gcsdrop

Publish an HTML file or directory to a Google Cloud Storage bucket you own, and get back a URL
you can paste to someone.

```bash
export GCSDROP_BUCKET=YOUR-BUCKET
gcsdrop ./report.html
```

```
https://storage.googleapis.com/YOUR-BUCKET/gcsdrop/20260904-000409-nkhmbc3x/report.html?X-Goog-Algorithm=...&X-Goog-Signature=...
```

That URL is a V4 signed link that expires in one hour by default; pass `--expires` for anything
up to 7 days. Anyone who has it can open the report; nobody needs a Google account.

The files land in **your** bucket, under **your** project, with your normal Cloud Audit Logs.
gcsdrop authenticates with Application Default Credentials and never asks you for a service
account key file.

```
 ./report-dir/                 gcsdrop                        gs://YOUR-BUCKET/
 ├─ index.html ─┐                                             └─ gcsdrop/<run-id>/
 ├─ style.css   │  1. walk, guess Content-Type                    ├─ index.html
 └─ chart.png   ├─▶ 2. read Application Default Credentials       ├─ style.css
                │  3. upload each file ───────────────────────▶   └─ chart.png
                │
                ├─▶ 4. sign the landing page
                │      ⚠️ signs ONE object — see "A signed URL covers one file" below
                │
                └─▶ 5. fetch that URL with no credentials, print it only on success
                      https://storage.googleapis.com/YOUR-BUCKET/...&X-Goog-Signature=...
```

### Configuration

gcsdrop has exactly two settings of its own, both environment variables. There is deliberately
no setting for a GCP project (bucket names are globally unique and the upload API does not take
one) and no setting for a signing service account (your credentials decide the identity).

| Variable | Required | Default | Notes |
|---|---|---|---|
| `GCSDROP_BUCKET` | yes | — | `my-bucket` or `gs://my-bucket`, both accepted |
| `GCSDROP_PREFIX` | no | `gcsdrop` | first path segment inside the bucket |

gcsdrop also honours `GOOGLE_APPLICATION_CREDENTIALS`, but that is the standard Google variable
every ADC-aware tool reads, not a gcsdrop setting. Leave it unset on GCE/GKE; the recommended
laptop setup sets it deliberately, scoped to one command — see
[Local development](#local-development).

Objects are named `<prefix>/<yyyymmdd-hhmmss>-<random8>/<relative path>`.

---

## Prerequisites

- **A GCP project and a bucket you own.** Everything lands in your own storage.
- **The `gcloud` CLI, for one-time setup only.** Creating the bucket and granting the two IAM
  roles are `gcloud` commands. If you do not have it, install it first:
  <https://cloud.google.com/sdk/docs/install>.

> To be clear about which is which: **gcsdrop itself never calls gcloud** and does not need it
> installed on the machine that runs the tool. Every `gcloud` command in this README is part of
> setting things up once. See [No gcloud at runtime](#no-gcloud-at-runtime).

---

## Install

Download a prebuilt binary. It is one self-contained executable with nothing to install
alongside it — no Rust, no Node, no Python, and **no gcloud** (see
[No gcloud at runtime](#no-gcloud-at-runtime)). The Linux builds are statically linked against
musl, so they do not care which glibc the host has.

**There is no `cargo install`.** Many container images have no `cargo` and no `rustc`, and a
tool you have to compile is a tool people will not try. Prebuilt binaries are the intended path;
[Build from source](#build-from-source) is the fallback.

Pick your platform's asset name and use it everywhere `ASSET` appears below:

| Platform | `ASSET` |
|---|---|
| Linux x86-64 | `gcsdrop-linux-amd64` |
| Linux arm64 | `gcsdrop-linux-arm64` |
| macOS Apple Silicon | `gcsdrop-macos-arm64` |

Each asset ships with a `.sha256` file next to it, built in the same release workflow run and
uploaded alongside the tarball. Checking it catches a corrupted or truncated download — it does
**not** prove the release itself is untampered, since both files come from the same run and an
attacker who could replace the tarball could replace the checksum beside it too. Still worth
doing.

### Today: the repository is private

`releases/latest/download/...` is an unauthenticated URL. GitHub returns a 404 for it on a
private repo — the same 404 whether the release is missing or you simply cannot see it. Use the
`gh` CLI instead; it authenticates the download for you, and `--pattern` fetches the tarball and
its checksum together.

**Prerequisite:** the [`gh` CLI](https://cli.github.com/), logged in (`gh auth login`), with
access to the `botrun` GitHub org. Without org access you get the same 404 as a missing release —
that is not a bug in this README, check your access before assuming the release is broken.

```bash
mkdir -p "$HOME/bin"
cd "$(mktemp -d)"

# Linux x86-64. On macOS use ASSET=gcsdrop-macos-arm64 and shasum -a 256 -c.
ASSET=gcsdrop-linux-amd64

gh release download --repo botrun/gcsdrop --pattern "$ASSET*"

sha256sum -c "$ASSET.tar.gz.sha256"      # macOS: shasum -a 256 -c "$ASSET.tar.gz.sha256"
tar -xzf "$ASSET.tar.gz" -C "$HOME/bin"
chmod +x "$HOME/bin/gcsdrop"
```

The tarball holds the bare `gcsdrop` binary with no wrapping directory, so `tar -xz -C ~/bin`
puts it exactly at `~/bin/gcsdrop`.

### Once the repository is public

⚠️ **Not yet — this fails with a 404 today**, for the private-repo reason explained above. Kept
here so that once the repo opens up, a reader lands on the simple `curl` form instead of being
sent to `gh release download` or the harder [Build from source](#build-from-source) path.

```bash
mkdir -p "$HOME/bin"
cd "$(mktemp -d)"

# Linux x86-64. On macOS use ASSET=gcsdrop-macos-arm64 and shasum -a 256 -c.
ASSET=gcsdrop-linux-amd64

curl -fsSLO "https://github.com/botrun/gcsdrop/releases/latest/download/$ASSET.tar.gz"
curl -fsSLO "https://github.com/botrun/gcsdrop/releases/latest/download/$ASSET.tar.gz.sha256"

sha256sum -c "$ASSET.tar.gz.sha256"      # macOS: shasum -a 256 -c "$ASSET.tar.gz.sha256"
tar -xzf "$ASSET.tar.gz" -C "$HOME/bin"
chmod +x "$HOME/bin/gcsdrop"
```

### Put it on your `PATH`

Every command in this README says `gcsdrop`, not a path, so the shell has to be able to find it:

```bash
export PATH="$HOME/bin:$PATH"
gcsdrop --version
```

```
gcsdrop 0.1.0
```

That `export` dies with the shell. To make it stick, add the same line to your shell profile —
`~/.zshrc` on macOS and most Linux setups, `~/.bashrc` if you use bash:

```bash
echo 'export PATH="$HOME/bin:$PATH"' >> ~/.zshrc
```

If you would rather not touch `PATH` at all, call the binary by its full path everywhere:
`~/bin/gcsdrop ./report.html`.

### Build from source

The fallback if a prebuilt binary does not fit — no `gh` CLI, no org access, or a platform not
in the table above. Requires a Rust toolchain (<https://rustup.rs>); it will not work in a
container image that ships without `cargo`. Cloning also requires the same GitHub access as the
prebuilt binaries, since the repository is private.

```bash
git clone https://github.com/botrun/gcsdrop
cd gcsdrop
cargo build --release
```

That leaves the binary at `target/release/gcsdrop`, which is **not** on your `PATH`. Copy it
somewhere that is, then carry on with [Put it on your `PATH`](#put-it-on-your-path):

```bash
mkdir -p "$HOME/bin"
cp target/release/gcsdrop "$HOME/bin/gcsdrop"
```

The release binary is about 13 MB.

---

## Create a bucket

**gcsdrop never creates a bucket.** Creating one means choosing a project, a location, a storage
class and a deletion policy — decisions that belong to you, not to a CLI flag. Do it once:

```bash
gcloud storage buckets create gs://YOUR-BUCKET \
  --project=YOUR-PROJECT \
  --location=asia-east1 \
  --uniform-bucket-level-access \
  --public-access-prevention
```

```
Creating gs://YOUR-BUCKET/...
```

- Pick your own `--location`. `asia-east1` is only an example.
- `--uniform-bucket-level-access` is what makes the permissions below behave predictably. Without
  it, per-object ACLs can override the bucket policy.
- `--public-access-prevention` blocks the bucket from ever being made public — by gcsdrop, by
  another tool, or by a person's slip. This bucket holds content shared only via short-lived
  signed links, and this setting makes it impossible to expose that content to the whole
  internet instead.

Then set it as your default:

```bash
export GCSDROP_BUCKET=YOUR-BUCKET
```

Like the `PATH` line earlier, that lasts only as long as the shell. Add it to `~/.zshrc` (or
`~/.bashrc`) if you do not want to retype it every session.

---

## Permissions

This is the section people get stuck in. Three roles matter, and **each one is bound to a
different resource**:

| Role | Bind it to | What it buys you |
|---|---|---|
| `roles/storage.objectCreator` | **the bucket**, not the project | uploading |
| `roles/storage.objectViewer` | **the bucket**, not the project | reading back what was uploaded — required for the signed URLs gcsdrop hands out to work |
| `roles/iam.serviceAccountTokenCreator` | **the service account itself** | signing URLs |

⚠️ **A valid signature is not enough.** A V4 signed URL is authorized as the identity that
signed it, not just by carrying a correct signature. `objectCreator` alone lets the signer
upload but not read, so every signed URL it produces — a GET request — comes back `403
Forbidden` for anyone you send it to, even though the upload itself succeeded. Grant
`objectViewer` in addition to `objectCreator`; do not use `roles/storage.objectAdmin` instead —
it also grants delete, which gcsdrop never needs (cleanup is a bucket [lifecycle
rule](#cleaning-up), not something this tool does).

⚠️ **`objectViewer` also grants `storage.objects.list`.** Any identity holding it can enumerate
and read every object in the bucket, not just the ones it uploaded itself. If several agents or
people share one service account — exactly the case gcsdrop was built for — they can all read
each other's uploads through it, and the random path segment gcsdrop puts in every object exists
specifically to make that enumeration useless, so `list` defeats it. `objectCreator` +
`objectViewer` is still the right default below for a service account gcsdrop has to itself. If
your service account is shared, skip to [A narrower role for a shared service
account](#a-narrower-role-for-a-shared-service-account) instead.

Set three shell variables, then paste the rest:

```bash
export PROJECT=YOUR-PROJECT
export BUCKET=YOUR-BUCKET
export SA=gcsdrop-signer@$PROJECT.iam.gserviceaccount.com
```

### 1. Create a service account to sign with

```bash
gcloud iam service-accounts create gcsdrop-signer \
  --project=$PROJECT \
  --display-name="gcsdrop signer"
```

Skip this if you already have a service account you want to use — on GCE or GKE, that is the
VM's own service account and you create nothing. See
[Running on GCE / GKE](#running-on-gce--gke).

### 2. Let the service account sign on its own behalf

```bash
gcloud iam service-accounts add-iam-policy-binding "$SA" --project=$PROJECT \
  --member="serviceAccount:$SA" \
  --role="roles/iam.serviceAccountTokenCreator"
```

Read that again: the member and the resource are **the same service account**. It looks like a
copy-paste mistake. It is not.

Here is why. A V4 signed URL has to be signed with the service account's private key, and the
only way to reach that key without holding a key file is Google's IAM `signBlob` API. In that
call the caller and the target are the same identity, so the service account needs
`serviceAccountTokenCreator` **on itself** to sign anything. This is true on every credential
path gcsdrop supports:

- On GCE and GKE, the auth library asks the metadata server for the account's email and then
  calls `signBlob` — the metadata server has no signing endpoint of its own.
- Under impersonation, it builds credentials *as* the service account and calls `signBlob`
  targeting that same account.

The one credential type that signs locally without this grant is a service account **key file**,
and gcsdrop does not support key files. So: grant it, or you will hit a `signBlob` 403 the first
time you run it.

### 3. Let it upload and read back — bind both on the bucket, not the project

```bash
gcloud storage buckets add-iam-policy-binding gs://$BUCKET \
  --member="serviceAccount:$SA" \
  --role="roles/storage.objectCreator"

gcloud storage buckets add-iam-policy-binding gs://$BUCKET \
  --member="serviceAccount:$SA" \
  --role="roles/storage.objectViewer"
```

Most tutorials online grant `objectCreator` at the project level. **Don't.** A project-level
`objectCreator` can write to every bucket in the project, including ones that have nothing to do
with sharing HTML reports. Bound to the bucket, the blast radius is the bucket.

Both roles are required, not just the first one. `objectCreator` alone lets gcsdrop upload but
not read — see the warning above about why that produces signed URLs that upload successfully
and then 403 for everyone they are shared with.

### A narrower role for a shared service account

Skip this if the service account above is dedicated to gcsdrop — `objectCreator` +
`objectViewer` is simpler and it is what most readers should use.

Use this instead if the service account is **shared** — several agents or services signing URLs
through one identity, which is exactly the situation gcsdrop was built for. `objectViewer`'s
`storage.objects.list` lets any of them enumerate and read every upload in the bucket, not just
their own. There is no predefined GCS role between "read one object you name" and "list and read
everything," so create a custom role with exactly the two permissions gcsdrop's signer uses:

```bash
gcloud iam roles create gcsdropPublisher \
  --project=$PROJECT \
  --title="gcsdrop publisher" \
  --permissions=storage.objects.create,storage.objects.get \
  --stage=GA
```

```bash
gcloud storage buckets add-iam-policy-binding gs://$BUCKET \
  --member="serviceAccount:$SA" \
  --role="projects/$PROJECT/roles/gcsdropPublisher"
```

Bind this role **instead of** `objectCreator` and `objectViewer` in [step
3](#3-let-it-upload-and-read-back--bind-both-on-the-bucket-not-the-project), not in addition to
them — adding `objectViewer` back brings `list` with it and undoes the point. Tested: a signer
holding only this custom role uploaded, signed, and produced a URL that fetched `200 OK`, with
no `storage.objects.list` granted anywhere.

```
$ gcloud iam roles describe gcsdropPublisher --project YOUR-PROJECT
includedPermissions:
- storage.objects.create
- storage.objects.get
name: projects/YOUR-PROJECT/roles/gcsdropPublisher
stage: GA
```

### Editors and Owners on the project can still read this bucket

Every GCS bucket also carries Google's own default bindings, independent of anything granted
above: `roles/storage.legacyObjectOwner` for `projectEditor:YOUR-PROJECT` and
`projectOwner:YOUR-PROJECT`. Confirmed on the bucket used to test this README — those bindings
were still present with the custom role above as the only other grant. **Anyone holding the
Editor or Owner role on the project can read and write every object in this bucket, whatever
role you bind to the signer.**

So the boundary that actually contains access is which project the bucket lives in and who has
Editor or Owner there — not the role tuning on this page. Give gcsdrop a bucket of its own, in a
project where you know everyone who holds Editor or Owner.

### Checking what you ended up with

```bash
gcloud iam service-accounts get-iam-policy "$SA" --project=$PROJECT
```

The self-grant from step 2 shows up as the service account listed under its own policy:

```yaml
bindings:
- members:
  - serviceAccount:gcsdrop-signer@YOUR-PROJECT.iam.gserviceaccount.com
  - user:you@example.com
  role: roles/iam.serviceAccountTokenCreator
```

And the bucket side:

```bash
gcloud storage buckets get-iam-policy gs://$BUCKET
```

### ➡️ Do not run gcsdrop yet if you are on a laptop

The grants above cover the *service account*. They do not change who **you** are. On a personal
machine your Application Default Credentials are still a user account, and gcsdrop will refuse
to sign:

```
Error: Your Application Default Credentials are a user account, which cannot sign URLs.
```

That is expected at this point, and nothing has been uploaded. Finish with
[Local development](#local-development) before your first run.

On GCE or GKE you can skip that — see [Running on GCE / GKE](#running-on-gce--gke) — and run the
tool now.

---

## Signed URL expiry

⚠️ **A signed URL is a bearer credential.** Anyone holding it can read the object until it
expires — no login, no identity check. Posting one into a public GitHub issue or public channel
publishes the content to whoever finds it. Think before you put a signed URL anywhere public.

```bash
gcsdrop ./report.html                        # default: signed URL, 1 hour
gcsdrop ./report.html --expires 30m          # spell it out
gcsdrop ./report.html --expires 7d           # 7d is Google's maximum
```

A directory upload sends every file and gives you the URL of `index.html` when there is one —
otherwise the URL of the first file it walked, so include an `index.html` if you care which page
people land on. Hidden files (anything starting with `.`) are skipped. The limits are 2000 files
and 512 MB per run; they exist to catch a mistyped path, not to ration you.

### ⚠️ A signed URL covers one file

A V4 signed URL is a signature over **one object**. Uploading a directory (`index.html` +
`style.css` + `chart.png`) gets you a signed URL for the landing page and nothing else: the
`style.css` and `chart.png` it references by relative path are separate objects with no
signature of their own, and the browser will fail to load them. You get an unstyled page with
broken images.

Ship one self-contained HTML file instead — inline the CSS in a `<style>` tag and embed images
as `data:` URIs. For AI-generated reports this is usually easy, and it is what makes a directory
upload unnecessary in the first place.

Single-file uploads are unaffected: one object, one signature, everything works.

### How gcsdrop verifies the link

A correct signature is not the same as a working link — see the permissions warning above about
`objectCreator` alone. So after uploading, gcsdrop fetches the URL itself, with no credentials,
before printing it:

```
 gcsdrop                                          storage.googleapis.com
 │
 ├─▶ upload the file(s)  ──────────────────────▶  object written
 │
 ├─▶ sign the landing page URL
 │
 ├─▶ GET that URL, no credentials  ─────────────▶  the exact request a
 │   (Range: bytes=0-0 — cheap, same auth path       recipient would make
 │    as a full GET, no meaningful latency)
 │
 ├─ 2xx ──▶ print the URL
 │
 └─ anything else ──▶ exit non-zero, explain why, print nothing
```

This is why an upload can succeed and gcsdrop still exits non-zero: the object landed in the
bucket, but the identity that signed the URL cannot read it back, so nobody you send that link
to could open it either. Printing it anyway would just move the failure from gcsdrop's exit code
to a confused message from whoever you shared it with. See [The run succeeds, prints a URL, and
the URL 403s](#the-run-succeeds-prints-a-url-and-the-url-403s) for the exact error and fix.

---

## Local development

On your laptop, `gcloud auth application-default login` gives you *user* credentials, and user
credentials **cannot sign URLs**. This is Google's limitation, not gcsdrop's — gcloud refuses
identically:

```
$ gcloud storage sign-url "gs://YOUR-BUCKET/probe.html" --project=YOUR-PROJECT --duration=1h
ERROR: (gcloud.storage.sign-url) This command requires a service account to sign a URL.
Please authenticate with a service account, or provide the global
'--impersonate-service-account' flag.
```

gcsdrop detects this before it uploads anything and tells you the same thing:

```
$ gcsdrop ./report.html
Error: Your Application Default Credentials are a user account, which cannot sign URLs.
...
Fix it by impersonating a service account:

    gcloud auth application-default login \
      --impersonate-service-account=SA@PROJECT.iam.gserviceaccount.com
```

So: impersonate the signer service account. Either way below, you first grant yourself
permission to impersonate it — this is on top of the self-grant from
[Permissions](#permissions), which lets the *service account* sign; this one lets *you* borrow
it. Reuses `$SA` and `$PROJECT` from [Permissions](#permissions) — if this is a fresh shell, set
them again:

```bash
export PROJECT=YOUR-PROJECT
export SA=gcsdrop-signer@$PROJECT.iam.gserviceaccount.com

gcloud iam service-accounts add-iam-policy-binding "$SA" --project=$PROJECT \
  --member="user:you@example.com" \
  --role="roles/iam.serviceAccountTokenCreator"
```

From there, two ways to actually run as that identity:

| | Setup | What it touches |
|---|---|---|
| [Scoped credentials file](#recommended-a-scoped-credentials-file) (recommended) | assemble a JSON file | only the gcsdrop command it's passed to |
| [Global login switch](#alternative-a-machine-wide-login-switch) | one `gcloud` command | every ADC-reading tool on the machine, until you switch back |

### Recommended: a scoped credentials file

Build a credentials file that wraps your existing user credentials as `source_credentials`, and
point `GOOGLE_APPLICATION_CREDENTIALS` at it **only for the gcsdrop command**. The machine's real
ADC file — `~/.config/gcloud/application_default_credentials.json`, the one every other GCP tool
reads — is never touched, so nothing else on the machine changes identity.

The file still authorizes through you: it wraps your own credentials as `source_credentials` and
just adds "use this identity to obtain tokens for that service account." This is not a service
account key file — there is no private key in it, only your refresh token and the service
account to impersonate.

`gcloud` has no flag that writes a file like this — `gcloud auth application-default login`
always writes to the well-known path above, never to a path you choose — so the file has to be
assembled by hand. Put it at `~/.config/gcsdrop/adc.json`, alongside `gcloud`'s own convention of
keeping its state under `~/.config/gcloud/`. Two permission levels matter, not just one:

```
~/.config/gcsdrop/          drwx------  (700)
~/.config/gcsdrop/adc.json  -rw-------  (600)
```

The directory needs `700` too, not just the file — the file holds a refresh token, and `700`
keeps other accounts on the same machine from even listing the directory to find it.

This is the command, verified end to end against a real bucket:

```bash
python3 -c "
import json, pathlib
src = json.load(open(pathlib.Path.home()/'.config/gcloud/application_default_credentials.json'))
SA = 'SA@PROJECT.iam.gserviceaccount.com'
cfg = {
    'type': 'impersonated_service_account',
    'service_account_impersonation_url':
        f'https://iamcredentials.googleapis.com/v1/projects/-/serviceAccounts/{SA}:generateAccessToken',
    'source_credentials': src,
    'delegates': [],
}
d = pathlib.Path.home()/'.config/gcsdrop'
d.mkdir(parents=True, exist_ok=True)
d.chmod(0o700)
out = d/'adc.json'
out.write_text(json.dumps(cfg, indent=2))
out.chmod(0o600)
print('wrote', out)
"
```

Edit the `SA = '...'` line to your own `$SA` before running it. Keep `mkdir(parents=True,
exist_ok=True)` and both `chmod` calls even though the directory will already exist most of the
time — you re-run this whole command every time your underlying user credentials expire, at
which point `~/.config/gcsdrop/` is already there, and dropping `exist_ok=True` would make that
rerun crash.

Then run gcsdrop with the variable set for that one command only:

```bash
GOOGLE_APPLICATION_CREDENTIALS=~/.config/gcsdrop/adc.json gcsdrop ./report.html --expires 7d
```

Verified result, from this path:

```
$ GCSDROP_BUCKET=YOUR-BUCKET \
  GOOGLE_APPLICATION_CREDENTIALS=~/.config/gcsdrop/adc.json \
  gcsdrop /tmp/local-t.html --expires 7d
exit=0
Uploading 1 file(s)...
Verifying the link works...

$ curl -sSI '<the printed URL>' | head -2
HTTP/2 200
content-type: text/html
```

The machine's own ADC file, `~/.config/gcloud/application_default_credentials.json`, was
confirmed still `authorized_user` (i.e. still you) after this ran.

The file expires when your underlying user credentials do. When it stops working, re-run
`gcloud auth application-default login` and regenerate the file with the command above.

### Alternative: a machine-wide login switch

Simpler — one command, no JSON to assemble — but it overwrites
`~/.config/gcloud/application_default_credentials.json`, the single ADC file every tool on the
machine reads. Every other GCP tool you run also becomes the service account, usually with far
narrower permissions than your own account, until you switch back.

**1. Keep a copy of your current credentials.** The next command overwrites them:

```bash
cp ~/.config/gcloud/application_default_credentials.json ~/adc-backup.json 2>/dev/null \
  || echo "no existing ADC to back up — nothing to lose"
```

That file only exists if you have run `gcloud auth application-default login` before. Plain
`gcloud auth login` does not create it, so "no existing ADC" is a perfectly normal answer here,
not a problem.

**2. Log in as the service account:**

```bash
gcloud auth application-default login \
  --impersonate-service-account=$SA
```

> ⚠️ **This is a machine-wide switch.** Application Default Credentials are a single file, so
> *every* tool on that machine that uses ADC becomes the service account until you switch back.
> To restore the credentials you backed up in step 1:
>
> ```bash
> cp ~/adc-backup.json ~/.config/gcloud/application_default_credentials.json
> ```
>
> If step 1 found nothing to back up, go back to being yourself with a plain
> `gcloud auth application-default login` — no `--impersonate-service-account` flag.

> ⚠️ **Do not set `GOOGLE_APPLICATION_CREDENTIALS` to make this alternative work.** `gcloud auth
> application-default login --help` says, verbatim: *"Do not set the
> `GOOGLE_APPLICATION_CREDENTIALS` environment variable if you want to use the credentials
> generated by this command in your local development."* That login always writes to
> `~/.config/gcloud/application_default_credentials.json`, never to whatever path the variable
> names. If the variable is already set and pointing somewhere stale, `unset` it. (This warning
> is about *this* alternative only — the scoped file above sets the variable on purpose, and
> points it at a file meant to be read that way.)

### Which credentials can sign

| Application Default Credentials | Signs? |
|---|---|
| none present → GCE/GKE metadata server | yes |
| `impersonated_service_account` | yes |
| `authorized_user` (plain `application-default login`) | **no** |
| `external_account` (Workload Identity Federation) | **no** |

gcsdrop checks this *before* uploading, so a run that cannot produce a URL leaves nothing behind
in your bucket.

---

## Troubleshooting

### `signBlob` 403 / `PERMISSION_DENIED`

The service account is not allowed to sign on its own behalf. This is
[grant 2 in Permissions](#2-let-the-service-account-sign-on-its-own-behalf):

```bash
gcloud iam service-accounts add-iam-policy-binding \
  gcsdrop-signer@YOUR-PROJECT.iam.gserviceaccount.com --project=YOUR-PROJECT \
  --member="serviceAccount:gcsdrop-signer@YOUR-PROJECT.iam.gserviceaccount.com" \
  --role="roles/iam.serviceAccountTokenCreator"
```

Yes, on itself. gcsdrop prints this same recipe when it sees the error.

### Upload 403

The identity cannot write to the bucket. Bind `roles/storage.objectCreator` **on the bucket**,
per [grant 3](#3-let-it-upload-and-read-back--bind-both-on-the-bucket-not-the-project):

```bash
gcloud storage buckets add-iam-policy-binding gs://YOUR-BUCKET \
  --member="serviceAccount:gcsdrop-signer@YOUR-PROJECT.iam.gserviceaccount.com" \
  --role="roles/storage.objectCreator"
```

While you are granting bucket IAM, also check the next entry — a missing `objectViewer` does
not show up here, it shows up later, after upload has already succeeded.

First check who gcsdrop actually is. Note that this is your **Application Default Credentials**
identity, which is not necessarily the account `gcloud auth list` shows — the two are configured
separately. On a laptop, look at the ADC file:

```bash
cat ~/.config/gcloud/application_default_credentials.json
```

`"type": "impersonated_service_account"` means you are the service account named in
`service_account_impersonation_url`. `"type": "authorized_user"` means you are still yourself,
and signed URLs will not work at all. On a VM, ask the metadata server:

```bash
curl -H "Metadata-Flavor: Google" \
  http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/email
```

### The run succeeds, prints a URL, and the URL 403s

gcsdrop checks for exactly this before printing a URL — see [How gcsdrop verifies the
link](#how-gcsdrop-verifies-the-link) — so as of this version you should see the failure at run
time, not from someone you shared the link with. If you are running an older build, or you hit
this from a URL gcsdrop already printed, this is the actual error GCS returns — captured while
testing this README, from a signer holding only `objectCreator`:

```
$ curl -sI '<the signed URL gcsdrop printed>'
HTTP/2 403
content-type: application/xml; charset=UTF-8

<?xml version='1.0' encoding='UTF-8'?><Error><Code>AccessDenied</Code>
<Message>Access denied.</Message><Details>SA@YOUR-PROJECT.iam.gserviceaccount.com
does not have storage.objects.get access to the Google Cloud Storage object.
Permission 'storage.objects.get' denied on resource
'//storage.googleapis.com/projects/_/buckets/YOUR-BUCKET/objects/gcsdrop/20260906-000919-6conn8rv/t.html'
(or it may not exist).</Details></Error>
```

The signer can create objects but not read them back: it has `roles/storage.objectCreator` and
not `roles/storage.objectViewer` (or the [narrower custom
role](#a-narrower-role-for-a-shared-service-account)). The upload genuinely succeeded — the
object is sitting in the bucket — but a V4 signed URL is authorized as the signer, so a GET link
needs the signer to hold `storage.objects.get` too. Grant it, per [grant
3](#3-let-it-upload-and-read-back--bind-both-on-the-bucket-not-the-project):

```bash
gcloud storage buckets add-iam-policy-binding gs://YOUR-BUCKET \
  --member="serviceAccount:gcsdrop-signer@YOUR-PROJECT.iam.gserviceaccount.com" \
  --role="roles/storage.objectViewer"
```

### The HTML downloads instead of rendering

The object's `Content-Type` is wrong. gcsdrop sets it from the file extension on upload, so this
usually means the file did not end in `.html`. Check the object:

```bash
gcloud storage objects describe gs://YOUR-BUCKET/gcsdrop/RUN-ID/report.html
```

```yaml
bucket: YOUR-BUCKET
content_type: text/html
...
```

If `content_type` is `application/octet-stream`, rename the file with a proper extension and
upload again.

### `GOOGLE_APPLICATION_CREDENTIALS points at ... which cannot be read`

That environment variable is set and names a file that is missing or unreadable. gcsdrop stops
rather than silently falling back, because the fallback would fail later with a much more
confusing error.

If you did not set this on purpose, the fix is to stop overriding ADC:

```bash
unset GOOGLE_APPLICATION_CREDENTIALS
```

gcsdrop then does normal ADC discovery and finds
`~/.config/gcloud/application_default_credentials.json` on a laptop, or the metadata server on a
VM.

If you *did* set it on purpose — the [scoped credentials
file](#recommended-a-scoped-credentials-file) pattern for local development — don't unset it;
check instead that the path is right and the file is still there.

⚠️ **Do not try to fix it by pointing the variable at a path and then logging in.** That login
writes to the well-known path above, never to the path the variable names. The full explanation
is in [Local development](#local-development).

### `GCSDROP_BUCKET is not set`

Exactly what it says — see [Create a bucket](#create-a-bucket). gcsdrop never creates one for
you.

---

## Cleaning up

**gcsdrop deletes nothing.** Every run adds a new `<prefix>/<timestamp>-<random>/` directory and
leaves it there. Left alone, a bucket used for daily reports grows forever.

Give the bucket a lifecycle rule instead. This one deletes anything older than 30 days:

```bash
cat > /tmp/gcsdrop-lifecycle.json <<'EOF'
{"lifecycle": {"rule": [{"action": {"type": "Delete"}, "condition": {"age": 30}}]}}
EOF

gcloud storage buckets update gs://YOUR-BUCKET \
  --lifecycle-file=/tmp/gcsdrop-lifecycle.json
```

Confirm it took:

```bash
gcloud storage buckets describe gs://YOUR-BUCKET
```

```yaml
lifecycle_config:
  rule:
  - action:
      type: Delete
    condition:
      age: 30
```

⚠️ The rule applies to the **whole bucket**, so use a bucket that only gcsdrop writes to.

To pull one upload down immediately, delete its run directory with
`gcloud storage rm -r gs://YOUR-BUCKET/gcsdrop/RUN-ID`. Signed URLs to it stop working at once.

---

## Running on GCE / GKE

Nothing to configure for *signing*. With no ADC file present, the auth library falls back to the
metadata server, which resolves to the VM's (or the workload's) service account, and that
account can sign. Install the binary, set `GCSDROP_BUCKET`, run it.

The service account still needs all three grants from [Permissions](#permissions):
`serviceAccountTokenCreator` on itself, `objectCreator` on the bucket, and `objectViewer` on the
bucket. Signing capability alone is not enough to produce a working link — see the ⚠️ warnings
in [Permissions](#permissions) about what happens with `objectCreator` but no `objectViewer`.
This was found by actually running gcsdrop against a GCE VM's service account that had
`objectCreator` but not `objectViewer`: the upload and the signature both succeeded, and the
resulting URL 403'd for every unauthenticated fetch.

> ⚠️ **A VM's service account is shared by everything running on that VM.** Every process, every
> container, every pod on the node inherits it. Granting it write access to your bucket grants
> that access to all of them — and because `objectViewer` also grants `storage.objects.list`,
> it grants read-and-enumerate access to everything already in the bucket, too.
>
> Check what else runs there before you bind anything. If the answer is "a lot", create a
> dedicated service account for gcsdrop and impersonate it instead of widening the VM's — or
> bind the [narrower custom role](#a-narrower-role-for-a-shared-service-account) instead of
> `objectViewer`, which is exactly the situation it is for.

Find out which identity you are actually running as:

```bash
curl -H "Metadata-Flavor: Google" \
  http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/email
```

Two container gotchas, both common with `readOnlyRootFilesystem: true`:

- Every directory on `PATH` may be read-only. Install into a writable path such as
  `$HOME/bin` — a PVC-backed home directory works, and a downloaded binary can be `chmod +x`'d
  and executed there.
- That directory is probably **not** on `PATH`. Either call the binary by absolute path
  (`$HOME/bin/gcsdrop ./report.html`) or `export PATH="$HOME/bin:$PATH"` first. "Just run
  `gcsdrop`" is not enough.

Workload Identity Federation is the one case that does *not* work for signed URLs:
`external_account` credentials cannot sign — not even with impersonation configured inside the
external_account block itself. The fix is to wrap that credential as the `source_credentials`
of an `impersonated_service_account` ADC config, with `service_account_impersonation_url` set
on the outer object; gcsdrop's own error message spells this out in full.

---

## Restricted networks

### Installing

Depends on which install method above you use. With `gh release download` — the one that works
today, since the repo is private — permit both, confirmed with `GH_DEBUG=api`:

```
api.github.com
release-assets.githubusercontent.com
```

`api.github.com` looks up the release and the asset's signed download URL; the bytes themselves
come from `release-assets.githubusercontent.com`.

With plain `curl` against `releases/latest/download/...` — only once the repo is public — permit
these two instead:

```
github.com
release-assets.githubusercontent.com
```

`github.com` serves the redirect; `release-assets.githubusercontent.com` serves the actual asset
bytes. Allowing only the first gets you a redirect you cannot follow.

### Running

At runtime gcsdrop talks to Google only. Which hosts depends on how it authenticates:

| Host | When |
|---|---|
| `storage.googleapis.com` | every upload; also what signed URLs point at |
| `iamcredentials.googleapis.com` | signing, via `signBlob` |
| `metadata.google.internal` | on GCE / GKE only, to discover the identity |
| `oauth2.googleapis.com` | **impersonation only** — see below |

⚠️ **That last one is the easy one to miss, and it is the one a laptop needs.** An impersonated
ADC has an ordinary user account as its *source* credential, and refreshing that source token
goes to `https://oauth2.googleapis.com/token`. So the local development setup this README
prescribes fails against an allowlist built from the first three hosts alone.

On GCE / GKE the identity comes from the metadata server and there is no source token to
refresh, so `oauth2.googleapis.com` is not needed there.

---

## No gcloud at runtime

Two different machines, two different answers — this is a claim about the second one:

| | Needs `gcloud`? |
|---|---|
| **Setting up**, once: create the bucket, grant the roles, configure a laptop's credentials | **yes** — see [Prerequisites](#prerequisites) |
| **Running the tool**, every time after that | **no** |

Once setup is done, **gcsdrop does not shell out to gcloud and does not need it installed.** It
reads Application Default Credentials itself and calls the Google APIs directly.

That is what makes it usable inside a container: a single static binary, `curl`'d into a
writable directory, with the identity coming from the metadata server. No SDK to install, no key
file to mount, no image to rebuild. The machine that runs gcsdrop and the machine you set it up
from do not have to be the same machine.

---

## License

MIT. See [LICENSE](LICENSE).
