"""Sign a trial build using the public OpenHarmony SDK test infrastructure."""
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time
import uuid

pki = Path(os.environ["OHOS_SDK_NATIVE"]).parent / "toolchains/lib"
source, target = [Path(arg).resolve() for arg in sys.argv[1:]]
password = "123456"  # Public SDK test store, as used by xtask/src/ohos.rs.
alias = "leaf trial debug"
chain_path = pki / "OpenHarmonyProfileDebug.pem"
pattern = r"-----BEGIN CERTIFICATE-----.*?-----END CERTIFICATE-----"
with tempfile.TemporaryDirectory() as tmp:
    work = Path(tmp)

    def run(*args):
        subprocess.run(["java", "-jar", pki / "hap-sign-tool.jar", *args], cwd=work, check=True)

    shutil.copyfile(pki / "OpenHarmony.p12", work / "debug.p12")
    store = ["-keystoreFile", "debug.p12", "-keystorePwd", password]
    chain = re.findall(pattern, chain_path.read_text(), re.S)
    if len(chain) != 3:
        raise ValueError("Unexpected profile certificate chain in SDK")
    for name, cert in zip(("root.cer", "ca.cer"), chain):
        (work / name).write_text(cert + "\n")
    run("generate-keypair", "-keyAlias", alias, "-keyPwd", password, "-keyAlg", "ECC",
        "-keySize", "NIST-P-256", *store)
    run("generate-app-cert", "-keyAlias", alias, "-keyPwd", password, "-signAlg", "SHA256withECDSA",
        "-issuer", "C=CN,O=OpenHarmony,OU=OpenHarmony Team,CN=OpenHarmony Application CA",
        "-issuerKeyAlias", "openharmony application ca", "-issuerKeyPwd", password,
        "-subject", "C=CN,O=KeepingRunning,OU=Trials,CN=Leaf Trial", "-validity", "365",
        "-rootCaCertFile", "root.cer", "-subCaCertFile", "ca.cer", "-outForm", "certChain",
        "-outFile", "app.cer", *store)
    cert = re.findall(pattern, (work / "app.cer").read_text(), re.S)[0]
    profile = json.loads((pki / "UnsgnedDebugProfileTemplate.json").read_text())
    now = int(time.time())
    profile.update(uuid=str(uuid.uuid4()), validity={"not-before": now, "not-after": now + 364 * 86400})
    profile["bundle-info"].update({"developer-id": "KeepingRunning", "bundle-name": "io.github.keepingrunning.leaftrial",
                                   "development-certificate": cert + "\n"})
    profile.pop("debug-info", None)
    (work / "profile.json").write_text(json.dumps(profile))
    run("sign-profile", "-mode", "localSign", "-keyAlias", "openharmony application profile debug",
        "-keyPwd", password, "-profileCertFile", chain_path, "-inFile", "profile.json",
        "-signAlg", "SHA256withECDSA", "-outFile", "profile.p7b", *store)
    run("verify-profile", "-inFile", "profile.p7b")
    run("sign-app", "-mode", "localSign", "-keyAlias", alias, "-keyPwd", password,
        "-appCertFile", "app.cer", "-profileFile", "profile.p7b", "-inFile", source,
        "-signAlg", "SHA256withECDSA", "-signCode", "1", "-outFile", target, *store)
    run("verify-app", "-inFile", target, "-outCertChain", "verified.cer", "-outProfile", "verified.p7b")
