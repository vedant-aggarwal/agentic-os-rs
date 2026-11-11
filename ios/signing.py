"""App Store signing for the Dioxus iOS build on a CI Mac, through the App Store Connect API.

An App Manager API key can't use Apple's cloud signing, so each run makes its own short-lived
Apple Distribution certificate and App Store profile, and removes both at the end. Builds already
uploaded stay valid.

  next-build   BUILD_NUMBER = highest build number of the app in App Store Connect + 1
  setup        certificate + App Store profile in a temporary keychain; writes SIGN_IDENTITY,
               PROFILE_PATH and ENTITLEMENTS to GITHUB_ENV
  cleanup      delete the profile and revoke the certificate made by setup

Env: ASC_KEY_ID, ASC_ISSUER_ID, ASC_KEY_PATH, BUNDLE_ID, APPLE_TEAM_ID, RUNNER_TEMP, GITHUB_ENV.
Nothing here prints a key, a certificate or an email.
"""
import base64, json, os, plistlib, re, subprocess, sys, time, urllib.error, urllib.request, uuid

import jwt  # PyJWT

API = "https://api.appstoreconnect.apple.com/v1"
TMP = os.environ.get("RUNNER_TEMP", "/tmp")
STATE = os.path.join(TMP, "signing-state.json")


def token():
    now = int(time.time())
    return jwt.encode({"iss": os.environ["ASC_ISSUER_ID"], "iat": now, "exp": now + 900, "aud": "appstoreconnect-v1"},
                      open(os.environ["ASC_KEY_PATH"]).read(), algorithm="ES256",
                      headers={"kid": os.environ["ASC_KEY_ID"], "typ": "JWT"})


def call(method, path, body=None):
    req = urllib.request.Request(API + path, method=method, data=json.dumps(body).encode() if body else None,
                                 headers={"Authorization": f"Bearer {token()}", "Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req) as r:
            raw = r.read()
            return json.loads(raw) if raw else {}
    except urllib.error.HTTPError as e:
        raise SystemExit(f"App Store Connect API {method} {path.split('?')[0]} failed ({e.code}): "
                         f"{e.read().decode(errors='replace')[:600]}")


def sh(*cmd):
    return subprocess.run(cmd, check=True, capture_output=True, text=True).stdout


def env_out(**kv):
    with open(os.environ["GITHUB_ENV"], "a") as f:
        for k, v in kv.items():
            f.write(f"{k}={v}\n")


def app_id():
    apps = call("GET", f"/apps?filter[bundleId]={os.environ['BUNDLE_ID']}")["data"]
    if not apps:
        raise SystemExit("No App Store Connect app has this bundle ID.")
    return apps[0]["id"]


def next_build():
    """Build numbers are unique per app across every version train, so take the highest one + 1."""
    app, best, path = app_id(), 0, None
    path = f"/builds?filter[app]={app}&limit=200&fields[builds]=version"
    while path:
        page = call("GET", path)
        for b in page["data"]:
            v = b["attributes"].get("version", "")
            if v.isdigit():
                best = max(best, int(v))
        nxt = page.get("links", {}).get("next")
        path = nxt[len(API):] if nxt else None
    floor = int(os.environ.get("BUILD_FLOOR", "0") or 0)
    number = max(best + 1, floor)
    print(f"Highest build in App Store Connect: {best}; this build: {number}")
    env_out(BUILD_NUMBER=number)


def setup():
    bundle, team = os.environ["BUNDLE_ID"], os.environ["APPLE_TEAM_ID"]
    key, csr = os.path.join(TMP, "dist.key"), os.path.join(TMP, "dist.csr")
    sh("openssl", "genrsa", "-out", key, "2048")
    sh("openssl", "req", "-new", "-key", key, "-out", csr, "-subj", "/CN=Agentic OS RS CI/C=IN")
    csr_text = "".join(l for l in open(csr).read().splitlines() if "CERTIFICATE REQUEST" not in l)

    cert = call("POST", "/certificates", {"data": {"type": "certificates",
        "attributes": {"certificateType": "DISTRIBUTION", "csrContent": csr_text}}})["data"]
    state = {"certificate": cert["id"]}
    json.dump(state, open(STATE, "w"))
    der, pem, p12 = (os.path.join(TMP, n) for n in ("dist.cer", "dist.pem", "dist.p12"))
    open(der, "wb").write(base64.b64decode(cert["attributes"]["certificateContent"]))

    kc, kc_pw, p12_pw = os.path.join(TMP, "ci.keychain-db"), uuid.uuid4().hex, uuid.uuid4().hex
    sh("openssl", "x509", "-inform", "DER", "-in", der, "-out", pem)
    sh("openssl", "pkcs12", "-export", "-legacy", "-inkey", key, "-in", pem, "-out", p12, "-passout", f"pass:{p12_pw}")
    sh("security", "create-keychain", "-p", kc_pw, kc)
    sh("security", "set-keychain-settings", "-lut", "21600", kc)
    sh("security", "unlock-keychain", "-p", kc_pw, kc)
    sh("security", "import", p12, "-k", kc, "-P", p12_pw, "-T", "/usr/bin/codesign", "-T", "/usr/bin/security")
    sh("security", "set-key-partition-list", "-S", "apple-tool:,apple:,codesign:", "-s", "-k", kc_pw, kc)
    existing = [l.strip().strip('"') for l in sh("security", "list-keychains", "-d", "user").splitlines()]
    sh("security", "list-keychains", "-d", "user", "-s", kc, *existing)
    ids = sh("security", "find-identity", "-v", "-p", "codesigning", kc)
    m = re.search(r"\b([0-9A-F]{40})\b", ids)
    if not m:
        raise SystemExit("The new certificate is not a usable signing identity.")
    os.remove(key)

    bundles = call("GET", f"/bundleIds?filter[identifier]={bundle}&limit=200")["data"]
    match = [b for b in bundles if b["attributes"]["identifier"] == bundle]
    if not match:
        raise SystemExit("The bundle ID is not registered in the Apple Developer account.")
    name = f"Agentic OS RS CI {int(time.time())}"
    prof = call("POST", "/profiles", {"data": {"type": "profiles",
        "attributes": {"name": name, "profileType": "IOS_APP_STORE"},
        "relationships": {"bundleId": {"data": {"type": "bundleIds", "id": match[0]["id"]}},
                          "certificates": {"data": [{"type": "certificates", "id": cert["id"]}]}}}})["data"]
    state["profile"] = prof["id"]
    json.dump(state, open(STATE, "w"))
    profile_path = os.path.join(TMP, "app.mobileprovision")
    open(profile_path, "wb").write(base64.b64decode(prof["attributes"]["profileContent"]))

    # The app asks for the minimum an App Store build needs; the profile allows a superset.
    granted = plistlib.loads(subprocess.run(["security", "cms", "-D", "-i", profile_path],
                                            check=True, capture_output=True).stdout).get("Entitlements", {})
    ent = {"application-identifier": f"{team}.{bundle}",
           "com.apple.developer.team-identifier": team,
           "get-task-allow": False,
           "keychain-access-groups": [f"{team}.{bundle}"]}
    if granted.get("beta-reports-active"):
        ent["beta-reports-active"] = True
    ent_path = os.path.join(TMP, "app.entitlements")
    with open(ent_path, "wb") as f:
        plistlib.dump(ent, f)
    print(f"Signing ready: certificate {cert['id']}, profile {prof['id']}; "
          f"entitlements {', '.join(sorted(ent))}")
    env_out(SIGN_IDENTITY=m.group(1), PROFILE_PATH=profile_path, ENTITLEMENTS=ent_path)


def cleanup():
    if not os.path.exists(STATE):
        return
    state = json.load(open(STATE))
    for kind, ident in (("profiles", state.get("profile")), ("certificates", state.get("certificate"))):
        if not ident:
            continue
        try:
            call("DELETE", f"/{kind}/{ident}")
            print(f"Removed {kind[:-1]} {ident}")
        except SystemExit as e:
            print(e)


def status():
    """The newest builds of the app and their processing state."""
    app = app_id()
    for b in call("GET", f"/builds?filter[app]={app}&sort=-uploadedDate&limit=3")["data"]:
        a = b["attributes"]
        print(f"build {a.get('version')} uploaded {a.get('uploadedDate')} state {a.get('processingState')}")


if __name__ == "__main__":
    {"next-build": next_build, "setup": setup, "cleanup": cleanup, "status": status}[sys.argv[1]]()
