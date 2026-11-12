"""App Store signing for the iOS builds on a CI Mac, through the App Store Connect API.

Two builds use it: the Dioxus app (one target) and the Capacitor phone shell in mobile/ (TARGETS=capacitor: the app
plus its widget, share, keyboard and, with FAMILY_CONTROLS=true, the three Screen Time extensions).

An App Manager API key can't use Apple's cloud signing, so each run makes its own short-lived
Apple Distribution certificate and App Store profile, and removes both at the end. Builds already
uploaded stay valid.

  next-build   BUILD_NUMBER = highest build number of the app in App Store Connect + 1
  setup        certificate + App Store profile in a temporary keychain; writes SIGN_IDENTITY,
               PROFILE_PATH and ENTITLEMENTS to GITHUB_ENV. With TARGETS=capacitor it registers every
               extension's bundle ID, turns on its capabilities, makes one profile per target and writes
               PROFILE_APP, PROFILE_WIDGETS, PROFILE_SHARE, PROFILE_KEYBOARD (+ Screen Time) instead.
  entitlements after ios/add_targets.rb (capacitor): each target's .entitlements cut to what its profile grants
  export-plist the exportOptions plist for xcodebuild -exportArchive (capacitor), path in EXPORT_PLIST
  cleanup      delete the profiles and revoke the certificate made by setup

Env: ASC_KEY_ID, ASC_ISSUER_ID, ASC_KEY_PATH, BUNDLE_ID, APPLE_TEAM_ID, RUNNER_TEMP, GITHUB_ENV, TARGETS,
FAMILY_CONTROLS.
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
    # Uploads that failed processing never become builds but still use up their number.
    try:
        for u in call("GET", f"/apps/{app}/buildUploads?limit=50")["data"]:
            v = u["attributes"].get("cfBundleVersion", "")
            if v.isdigit():
                best = max(best, int(v))
    except SystemExit as e:
        print(f"buildUploads not readable, using builds only: {str(e)[:200]}")
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
    # The WWDR intermediate must be findable, or codesign embeds only the leaf and App Store Connect rejects
    # the signature (ITMS-90034/90035). Runner images usually have it; import it anyway.
    for ca in ("AppleWWDRCAG3.cer", "AppleWWDRCAG4.cer"):
        dest = os.path.join(TMP, ca)
        try:
            urllib.request.urlretrieve(f"https://www.apple.com/certificateauthority/{ca}", dest)
            subprocess.run(["security", "import", dest, "-k", kc], check=False, capture_output=True)
        except Exception as e:
            print(f"Could not fetch {ca}: {e}")
    existing = [l.strip().strip('"') for l in sh("security", "list-keychains", "-d", "user").splitlines()]
    sh("security", "list-keychains", "-d", "user", "-s", kc, *existing)
    ids = sh("security", "find-identity", "-v", "-p", "codesigning", kc)
    m = re.search(r"\b([0-9A-F]{40})\b", ids)
    if not m:
        raise SystemExit("The new certificate is not a usable signing identity.")
    kind = re.search(r'"(Apple Distribution|iPhone Distribution|Apple Development|iPhone Developer)', ids)
    print(f"Signing identity type: {kind.group(1) if kind else 'unknown'}")
    if not kind or "Distribution" not in kind.group(1):
        raise SystemExit("The new certificate is not an Apple Distribution identity.")
    os.remove(key)

    if os.environ.get("TARGETS", "").lower() == "capacitor":
        setup_targets(bundle, cert["id"], state)
        env_out(SIGN_IDENTITY=m.group(1), SIGN_KEYCHAIN=kc)
        return

    bundles = call("GET", f"/bundleIds?filter[identifier]={bundle}&limit=200")["data"]
    match = [b for b in bundles if b["attributes"]["identifier"] == bundle]
    if not match:
        raise SystemExit("The bundle ID is not registered in the Apple Developer account.")
    ensure_capabilities(match[0]["id"])
    name = f"Agentic OS RS CI {int(time.time())}"
    prof = call("POST", "/profiles", {"data": {"type": "profiles",
        "attributes": {"name": name, "profileType": "IOS_APP_STORE"},
        "relationships": {"bundleId": {"data": {"type": "bundleIds", "id": match[0]["id"]}},
                          "certificates": {"data": [{"type": "certificates", "id": cert["id"]}]}}}})["data"]
    state["profile"] = prof["id"]
    json.dump(state, open(STATE, "w"))
    profile_path = os.path.join(TMP, "app.mobileprovision")
    content = base64.b64decode(prof["attributes"]["profileContent"])
    open(profile_path, "wb").write(content)
    # Installed where xcodebuild -exportArchive looks for profiles (Xcode 16+ and the older folder).
    uid = prof["attributes"]["uuid"]
    for d in ("~/Library/Developer/Xcode/UserData/Provisioning Profiles", "~/Library/MobileDevice/Provisioning Profiles"):
        d = os.path.expanduser(d)
        os.makedirs(d, exist_ok=True)
        open(os.path.join(d, f"{uid}.mobileprovision"), "wb").write(content)

    # The app asks for the minimum an App Store build needs; the profile allows a superset.
    granted = plistlib.loads(subprocess.run(["security", "cms", "-D", "-i", profile_path],
                                            check=True, capture_output=True).stdout).get("Entitlements", {})
    ent = {"application-identifier": f"{team}.{bundle}",
           "com.apple.developer.team-identifier": team,
           "get-task-allow": False,
           "keychain-access-groups": [f"{team}.{bundle}"]}
    if granted.get("beta-reports-active"):
        ent["beta-reports-active"] = True
    # Push (APNs) and time-sensitive notifications, when the App ID has them. TestFlight and the
    # App Store use the production APNs environment.
    if granted.get("aps-environment"):
        ent["aps-environment"] = "production"
    if granted.get("com.apple.developer.usernotifications.time-sensitive"):
        ent["com.apple.developer.usernotifications.time-sensitive"] = True
    missing = [k for k in ("aps-environment", "com.apple.developer.usernotifications.time-sensitive") if k not in ent]
    if missing:
        print(f"Warning: the App Store profile does not grant {', '.join(missing)}; enable it on the App ID "
              f"at developer.apple.com (Identifiers > the app > Capabilities).")
    ent_path = os.path.join(TMP, "app.entitlements")
    with open(ent_path, "wb") as f:
        plistlib.dump(ent, f)
    print(f"Signing ready: certificate {cert['id']}, profile {prof['id']}; "
          f"entitlements {', '.join(sorted(ent))}")
    env_out(SIGN_IDENTITY=m.group(1), PROFILE_PATH=profile_path, ENTITLEMENTS=ent_path,
            PROFILE_NAME=name, PROFILE_UUID=uid, SIGN_KEYCHAIN=kc)


def ensure_capabilities(bundle_id_resource):
    """Push Notifications and Time Sensitive Notifications on the App ID, so new profiles carry
    aps-environment. Best effort: an API key without the right role only gets a warning."""
    try:
        have = {c["attributes"].get("capabilityType")
                for c in call("GET", f"/bundleIds/{bundle_id_resource}/bundleIdCapabilities")["data"]}
    except SystemExit as e:
        print(f"Could not read the App ID capabilities: {str(e)[:200]}")
        return
    print(f"App ID capabilities: {', '.join(sorted(t for t in have if t)) or 'none'}")
    for cap in ("PUSH_NOTIFICATIONS", "USERNOTIFICATIONS_TIMESENSITIVE"):
        if cap in have:
            continue
        try:
            call("POST", "/bundleIdCapabilities", {"data": {"type": "bundleIdCapabilities",
                 "attributes": {"capabilityType": cap},
                 "relationships": {"bundleId": {"data": {"type": "bundleIds", "id": bundle_id_resource}}}}})
            print(f"Enabled {cap} on the App ID")
        except SystemExit as e:
            print(f"Could not enable {cap}: {str(e)[:200]}")


# --- Capacitor shell: one profile per target -------------------------------------------------------------

# Where each target's entitlements live under mobile/ios/App (ios/add_targets.rb uses the same names).
TARGET_ENTITLEMENTS = {"APP": "App/App.entitlements", "WIDGETS": "AgenticWidgets/AgenticWidgets.entitlements",
                       "SHARE": "AgenticShare/AgenticShare.entitlements",
                       "KEYBOARD": "AgenticKeyboard/AgenticKeyboard.entitlements",
                       "MONITOR": "AgenticMonitor/AgenticMonitor.entitlements",
                       "SHIELD": "AgenticShield/AgenticShield.entitlements",
                       "SHIELDACTION": "AgenticShieldAction/AgenticShieldAction.entitlements"}
SHELL_IOS = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "mobile", "ios", "App")


def shell_targets(bundle):
    """(env key, bundle id, name, capabilities) for the app and its extensions. A capability is a list of App Store
    Connect names tried in order; a leading "?" makes it optional (refused: the run goes on and the entitlements
    step drops it). The shared keychain group needs no capability: App Store profiles allow TEAMID.*."""
    family = os.environ.get("FAMILY_CONTROLS", "").lower() == "true"
    app_caps = [["PUSH_NOTIFICATIONS"], ["?HEALTHKIT"],
                ["?USERNOTIFICATIONS_TIMESENSITIVE", "?TIME_SENSITIVE_NOTIFICATIONS"]]
    if family:
        app_caps.append(["?FAMILY_CONTROLS"])
    out = [("APP", bundle, "Agentic OS", app_caps),
           ("WIDGETS", f"{bundle}.widgets", "Agentic OS Widgets", []),
           ("SHARE", f"{bundle}.share", "Agentic OS Share", []),
           ("KEYBOARD", f"{bundle}.keyboard", "Agentic OS Keyboard", [])]
    if family:
        out += [("MONITOR", f"{bundle}.monitor", "Agentic OS Screen Time Monitor", [["?FAMILY_CONTROLS"]]),
                ("SHIELD", f"{bundle}.shield", "Agentic OS Shield", [["?FAMILY_CONTROLS"]]),
                ("SHIELDACTION", f"{bundle}.shieldaction", "Agentic OS Shield Action", [["?FAMILY_CONTROLS"]])]
    return out


def ensure_bundle(ident, label):
    ids = call("GET", f"/bundleIds?filter[identifier]={ident}&limit=200")["data"]
    match = [b for b in ids if b["attributes"]["identifier"] == ident]
    if match:
        return match[0]["id"]
    made = call("POST", "/bundleIds", {"data": {"type": "bundleIds",
        "attributes": {"identifier": ident, "name": label, "platform": "IOS"}}})["data"]
    print(f"Registered bundle ID for {label}")
    return made["id"]


def ensure_capability(pk, names):
    """Turns on the first of names the API accepts. Optional names ("?X") warn instead of failing the run."""
    have = {c["attributes"]["capabilityType"] for c in call("GET", f"/bundleIds/{pk}/bundleIdCapabilities")["data"]}
    if have.intersection(n.lstrip("?") for n in names):
        return True
    for raw in names:
        cap = raw.lstrip("?")
        try:
            call("POST", "/bundleIdCapabilities", {"data": {"type": "bundleIdCapabilities",
                 "attributes": {"capabilityType": cap},
                 "relationships": {"bundleId": {"data": {"type": "bundleIds", "id": pk}}}}})
            print(f"Turned on {cap}")
            return True
        except SystemExit as e:
            if not raw.startswith("?"):
                raise
            print(f"Could not turn on {cap} (left out of this build): {str(e)[:300]}")
    return False


def profile_keys(path):
    raw = subprocess.run(["security", "cms", "-D", "-i", path], check=True, capture_output=True).stdout
    return sorted(plistlib.loads(raw).get("Entitlements", {}).keys())


def setup_targets(bundle, cert_id, state):
    stamp = int(time.time())
    state["profiles"], state["granted"], names = [], {}, {}
    dirs = [os.path.expanduser(d) for d in ("~/Library/Developer/Xcode/UserData/Provisioning Profiles",
                                             "~/Library/MobileDevice/Provisioning Profiles")]
    for d in dirs:
        os.makedirs(d, exist_ok=True)
    env = {}
    for key, ident, label, caps in shell_targets(bundle):
        pk = ensure_bundle(ident, label)
        for cap in caps:
            ensure_capability(pk, cap)
        name = f"{label} CI {stamp}"
        prof = call("POST", "/profiles", {"data": {"type": "profiles",
            "attributes": {"name": name, "profileType": "IOS_APP_STORE"},
            "relationships": {"bundleId": {"data": {"type": "bundleIds", "id": pk}},
                              "certificates": {"data": [{"type": "certificates", "id": cert_id}]}}}})["data"]
        state["profiles"].append(prof["id"])
        json.dump(state, open(STATE, "w"))
        content = base64.b64decode(prof["attributes"]["profileContent"])
        for d in dirs:
            open(os.path.join(d, f"{prof['attributes']['uuid']}.mobileprovision"), "wb").write(content)
        path = os.path.join(dirs[0], f"{prof['attributes']['uuid']}.mobileprovision")
        state["granted"][key] = profile_keys(path)
        names[ident] = name
        json.dump(state, open(STATE, "w"))
        env[f"PROFILE_{key}"] = name
        print(f"Profile for {key}: grants {', '.join(state['granted'][key])}")
    state["names"] = names
    json.dump(state, open(STATE, "w"))
    env_out(**env)
    print(f"Signing ready: certificate {cert_id}, {len(state['profiles'])} profiles")


def entitlements():
    """After ios/add_targets.rb: each target's entitlements, cut to what its profile grants, so a capability Apple
    has not approved (Family Controls before the request is granted, HealthKit) never breaks signing."""
    if not os.path.exists(STATE):
        print("No signing state: entitlements left as they are.")
        return
    state = json.load(open(STATE))
    for key, allowed in state.get("granted", {}).items():
        path = os.path.join(SHELL_IOS, TARGET_ENTITLEMENTS[key])
        if not os.path.exists(path):
            continue
        with open(path, "rb") as f:
            requested = plistlib.load(f)
        allowed = set(allowed)
        kept = {k: v for k, v in requested.items()
                if k in allowed or (k == "com.apple.developer.healthkit.access" and "com.apple.developer.healthkit" in allowed)}
        dropped = sorted(set(requested) - set(kept))
        with open(path, "wb") as f:
            plistlib.dump(kept, f)
        print(f"{key}: keeps {', '.join(sorted(kept)) or 'nothing'}" + (f"; drops {', '.join(dropped)}" if dropped else ""))


def export_plist():
    """exportOptions for xcodebuild -exportArchive: app-store-connect, manual signing, every target's profile."""
    state = json.load(open(STATE))
    opts = {"method": "app-store-connect", "destination": "export", "teamID": os.environ["APPLE_TEAM_ID"],
            "signingStyle": "manual", "signingCertificate": "Apple Distribution",
            "provisioningProfiles": state["names"], "uploadSymbols": False,
            "manageAppVersionAndBuildNumber": False}
    path = os.path.join(TMP, "export.plist")
    with open(path, "wb") as f:
        plistlib.dump(opts, f)
    print(f"Export options: {len(state['names'])} profiles")
    env_out(EXPORT_PLIST=path)


def cleanup():
    if not os.path.exists(STATE):
        return
    state = json.load(open(STATE))
    gone = [("profiles", p) for p in state.get("profiles", [])]
    gone += [("profiles", state.get("profile")), ("certificates", state.get("certificate"))]
    for kind, ident in gone:
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


def wait_processing():
    """Waits for Apple to finish processing this upload; fails the run on processing errors (ITMS-…)."""
    app, want = app_id(), os.environ["BUILD_NUMBER"]
    deadline = time.time() + int(os.environ.get("PROCESSING_TIMEOUT", "1800"))
    last = None
    while time.time() < deadline:
        ups = [u["attributes"] for u in call("GET", f"/apps/{app}/buildUploads?limit=50")["data"]
               if u["attributes"].get("cfBundleVersion") == want]
        if ups:
            st = ups[0].get("state") or {}
            state = st.get("state")
            if state != last:
                print(f"Upload of build {want}: {state}")
                last = state
            for w in st.get("warnings") or []:
                print(f"  warning {w.get('code')}: {w.get('description')}")
            if state == "FAILED":
                for e in st.get("errors") or []:
                    print(f"  error {e.get('code')}: {e.get('description')}")
                raise SystemExit(f"App Store Connect rejected build {want}.")
            if state == "COMPLETE":
                builds = call("GET", f"/builds?filter[app]={app}&filter[version]={want}")["data"]
                if builds and builds[0]["attributes"].get("processingState") == "VALID":
                    print(f"Build {want} is VALID in TestFlight.")
                    return
                if builds:
                    print(f"Build {want}: {builds[0]['attributes'].get('processingState')}")
        time.sleep(30)
    print(f"Still processing after the timeout; check App Store Connect for build {want}.")


if __name__ == "__main__":
    {"next-build": next_build, "setup": setup, "cleanup": cleanup, "status": status,
     "wait-processing": wait_processing, "entitlements": entitlements,
     "export-plist": export_plist}[sys.argv[1]]()
