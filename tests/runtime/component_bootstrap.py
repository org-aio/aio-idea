"""正式产品、真实 Wasm 与独立 PostgreSQL 验证管理整包引导、信任撤销及恢复。"""

import gzip
import json
import os
import shutil
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from pathlib import Path


binary = Path(os.environ["AIO_BOOTSTRAP_TEST_BINARY"]).resolve(strict=True)
packager = Path(os.environ["AIO_BOOTSTRAP_TEST_PACKAGER"]).resolve(strict=True)
wasm = Path(os.environ["AIO_BOOTSTRAP_TEST_WASM_COMPONENT"]).resolve(strict=True)
database = os.environ["AIO_BOOTSTRAP_TEST_DATABASE_URL"]
postgres = os.environ["AIO_BOOTSTRAP_TEST_POSTGRES_CONTAINER"]
parsed = urllib.parse.urlparse(database)
assert parsed.hostname == "127.0.0.1" and parsed.path == "/component_bootstrap_test"
root = Path(tempfile.mkdtemp(prefix="aio-component-bootstrap-product-"))
config = root / "deployment/aio.toml"
config.parent.mkdir()
(config.parent / "bundles").mkdir()
(root / "web").mkdir()
(root / "web/index.html").write_text("<!doctype html><title>Bootstrap test</title>", encoding="utf-8")
source = root / "composition.json"
record = root / "cache/runtime-composition/record.json"
with socket.socket() as reservation:
    reservation.bind(("127.0.0.1", 0))
    port = reservation.getsockname()[1]
base = "http://127.0.0.1:%d" % port
env = {key: value for key, value in os.environ.items() if not key.startswith("AIO_")}
env.update({
    "AIO_DATABASE_URL": database, "AIO_COMPONENT_DATABASE_URL": database,
    "AIO_COMPONENT_HOME": str(root / "components"), "AIO_PLUGIN_CACHE": str(root / "cache"),
    "AIO_CONFIG": str(config), "AIO_FILE_STORAGE_DIR": str(root / "files"),
    "AIO_PROCESS_ROOT": str(root / "processes"),
    "AIO_PLUGIN_SUPERVISOR_SOCKET": str(root / "absent-supervisor.sock"),
    "AIO_RUNTIME_COMPOSITION": str(source), "AIO_PUBLIC_ORIGIN": base,
    "AIO_WEB_HOST": "127.0.0.1", "AIO_WEB_PORT": str(port), "AIO_WEB_DIST": str(root / "web"),
    "AIO_BOOTSTRAP_ACCOUNT": "bootstrap-test", "AIO_BOOTSTRAP_PASSWORD": uuid.uuid4().hex,
    "AIO_PLUGIN_PUBLISH_ACCOUNTS": "bootstrap-test",
})
http = urllib.request.build_opener(urllib.request.ProxyHandler({}))
cookie = None
host = None
log = open(str(root / "server.log"), "w", encoding="utf-8")
git = "https://github.com/example/component-bootstrap.git"


def sql(query):
    return subprocess.check_output(["docker", "exec", postgres, "psql", "-U", "postgres",
        "-d", "component_bootstrap_test", "-X", "-At", "-v", "ON_ERROR_STOP=1", "-c", query],
        universal_newlines=True).strip()


def request(path, value=None, raw=None):
    headers = {"Cookie": cookie} if cookie else {}
    body = raw
    if value is not None:
        body = json.dumps(value).encode()
        headers["Content-Type"] = "application/json"
    elif raw is not None:
        headers["Content-Type"] = "application/vnd.aio.component+gzip"
    try:
        response = http.open(urllib.request.Request(base + path, data=body, headers=headers), timeout=10)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        return response.status, response.headers, response.read()


def success(path, value=None, raw=None):
    status, _, body = request(path, value, raw)
    assert 200 <= status < 300, (path, status, body)
    result = json.loads(body)
    return result.get("data", result)


def until(check):
    deadline = time.monotonic() + 50
    while time.monotonic() < deadline:
        result = check()
        if result:
            return result
        time.sleep(0.1)
    raise AssertionError("等待超时；日志目录: " + str(root))


def ready():
    assert host.poll() is None, "宿主提前退出；日志目录: " + str(root)
    try:
        return request("/health")[0] == 200
    except OSError:
        return False


def launch():
    global host
    host = subprocess.Popen([str(binary)], cwd=str(root), env=env, stdout=log, stderr=subprocess.STDOUT)
    until(ready)
    # 市场系统服务已移除，整包引导仍能运行。
    assert request("/api/plugins/marketplace")[0] == 404


def stop():
    global host
    if host is not None and host.poll() is None:
        host.terminate()
        assert host.wait(timeout=40) == 0
    host = None


def configure(enabled, trusted=()):
    composition = {"schema_version": 1, "wasm": {"enabled": enabled},
        **{key: {"enabled": False} for key in ["sidecar", "rhai", "typescript", "evolution",
                                               "files", "access_control", "tenants"]},
        "trusted_management_components": [{"git": git, "digest": bundle[1]} for bundle in trusted]}
    pending = source.with_suffix(".pending")
    pending.write_text(json.dumps(composition), encoding="utf-8")
    pending.replace(source)
    if host is not None:
        def applied():
            value = json.loads(record.read_text(encoding="utf-8"))["confirmed"]["value"]
            return value["wasm"]["enabled"] == enabled and value["trusted_management_components"] == composition["trusted_management_components"]
        until(applied)


def package(version, repository=git, management=False):
    candidate = root / ("wasm-" + version)
    (candidate / "web").mkdir(parents=True)
    (candidate / "web/index.html").write_text("<!doctype html><title>Fixture</title>", encoding="utf-8")
    shutil.copy2(str(wasm), str(candidate / "plugin.wasm"))
    permissions = "required_permissions=['plugin:manage']\n[plugin.capabilities]\nmanagement=true\n" if management else ""
    (candidate / "aio-plugin.toml").write_text(
        "schema_version=2\n[plugin]\npermissions=['fixture.view']\n" + permissions + "[plugin.marketplace]\n"
        "title='Component bootstrap'\nsummary='First install fixture'\nlicense='MIT'\n"
        "[plugin.runtime]\nartifact='plugin.wasm'\nhost_version='>=2026.10.16'\n"
        "[plugin.frontend]\npath='web'\n", encoding="utf-8")
    output = config.parent / ("bundles/wasm-" + version + ".aio-plugin")
    subprocess.check_call([str(packager), str(candidate), "aio-plugin.toml", repository, "a" * 40, version, str(output)])
    digest = json.loads(gzip.decompress(output.read_bytes()))["digest"]
    return output, digest


def declaration(bundle, digest=None):
    path, revision = bundle
    return "\n[[bootstrap_components]]\npath='%s'\ndigest='%s'\ntenants=['default']\n" % (
        path.relative_to(config.parent), digest or revision)


def installation():
    return sql("SELECT row_to_json(i) FROM component_installations i JOIN component_sources s ON s.id=i.source_id WHERE s.git='%s' AND i.tenant_id='default'" % git)


def invoke(source_id):
    token = success("/api/runtime/frontend/mount", {"page_id": "component:%s:fixture" % source_id})["token"]
    result = success("/api/runtime/components/%s/request" % token,
                     {"method": "POST", "path": "/echo", "body": [52, 50], "headers": []})
    assert result["status"] == 200 and bytes(result["body"]) == b"42", result


def manage(source_id, path, value=None):
    token = success("/api/runtime/frontend/mount", {"page_id": "component:%s:fixture" % source_id})["token"]
    result = success("/api/runtime/components/%s/manage" % token, {
        "method": "GET" if value is None else "POST", "path": path,
        "body": [] if value is None else list(json.dumps(value).encode()),
        "headers": [{"name": "content-type", "value": "application/json"}],
    })
    assert 200 <= result["status"] < 300, result
    return token


try:
    assert sql("SELECT count(*) FROM pg_tables WHERE schemaname='public'") == "0", "必须使用新的独立测试库"
    first = package("1.0.0", management=True)
    second = package("2.0.0", management=True)
    config.write_text("plugins=[]\n" + declaration(first), encoding="utf-8")
    configure(False, [first])
    launch()
    assert sql("SELECT count(*) FROM component_initializations") == "0"
    assert sql("SELECT count(*) FROM component_sources") == "0"
    status, headers, _ = request("/api/auth/login", {"account": env["AIO_BOOTSTRAP_ACCOUNT"], "password": env["AIO_BOOTSTRAP_PASSWORD"]})
    assert status == 200
    cookie = headers["Set-Cookie"].split(";")[0]
    configure(True, [first])
    source_id = sql("SELECT id FROM component_sources WHERE git='%s'" % git)
    invoke(source_id)
    manage(source_id, "/api/runtime/catalog")
    installed = installation()
    assert json.loads(installed)["digest"] == first[1]
    # 首次安装后强杀真实宿主，下一进程不得重复激活或改变代数。
    host.kill()
    assert host.wait(timeout=10) != 0
    host = None
    launch()
    assert installation() == installed
    invoke(source_id)

    assert request("/api/runtime/components/publish", raw=second[0].read_bytes())[0] >= 400
    configure(True, [first, second])
    success("/api/runtime/components/publish", raw=second[0].read_bytes())
    old_token = manage(source_id, "/api/runtime/plugins/install", {"git": git, "rev": second[1]})
    assert request("/api/runtime/components/%s/manage" % old_token, {"method": "GET", "path": "/api/runtime/catalog"})[0] >= 400
    manage(source_id, "/api/runtime/plugins/%s/rollback" % source_id, {})
    rolled_back = installation()
    assert json.loads(rolled_back)["excluded_digest"] == second[1]
    stop()
    launch()
    assert installation() == rolled_back
    assert sql("SELECT digest FROM component_publications WHERE source_id='%s'" % source_id) == second[1]
    invoke(source_id)

    # 撤去部署信任后首次引导不能重新授权；重启仍暂停，恢复信任不改变租户选择。
    old_token = manage(source_id, "/api/runtime/catalog")
    configure(True)
    assert installation() == rolled_back
    assert request("/api/runtime/components/%s/manage" % old_token, {"method": "GET", "path": "/api/runtime/catalog"})[0] >= 400
    assert request("/api/runtime/frontend/mount", {"page_id": "component:%s:fixture" % source_id})[0] >= 400
    stop()
    launch()
    assert installation() == rolled_back
    assert request("/api/runtime/frontend/mount", {"page_id": "component:%s:fixture" % source_id})[0] >= 400
    configure(True, [first, second])
    assert installation() == rolled_back
    invoke(source_id)
    manage(source_id, "/api/runtime/catalog")

    # 无效信任候选不得替换已确认组合，下一进程从最后成功的声明恢复。
    rejected = json.loads(source.read_text(encoding="utf-8"))
    rejected["trusted_management_components"][0]["digest"] = "latest"
    pending = source.with_suffix(".pending")
    pending.write_text(json.dumps(rejected), encoding="utf-8")
    pending.replace(source)
    until(lambda: (json.loads(record.read_text(encoding="utf-8"))["attempt"] or {}).get("rejected", False))
    assert installation() == rolled_back
    manage(source_id, "/api/runtime/catalog")
    stop()
    launch()
    manage(source_id, "/api/runtime/catalog")
    configure(True, [first, second])

    manage(source_id, "/api/runtime/plugins/%s/disable" % source_id, {})
    disabled = installation()
    stop()
    launch()
    assert installation() == disabled and not json.loads(disabled)["enabled"]
    assert request("/api/runtime/frontend/mount", {"page_id": "component:%s:fixture" % source_id})[0] >= 400
    success("/api/runtime/plugins/%s/enable" % source_id, {})
    manage(source_id, "/api/runtime/plugins/%s/uninstall" % source_id, {})
    stop()
    config.write_text("plugins=[]\n" + declaration(second), encoding="utf-8")
    launch()
    assert installation() == ""
    assert sql("SELECT count(*) FROM component_initializations WHERE tenant_id='default' AND git='%s'" % git) == "1"
    # 明确手动安装仍然可行；撤掉部署引导配置不能隐式卸载。
    success("/api/runtime/plugins/install", {"git": git, "rev": second[1]})
    manual = installation()
    stop()
    config.write_text("plugins=[]\n", encoding="utf-8")
    launch()
    assert installation() == manual
    invoke(source_id)
    stop()

    third_git = "https://github.com/example/bootstrap-batch.git"
    third = package("3.0.0", third_git)
    config.write_text("plugins=[]\n" + declaration(third) + declaration(first, "0" * 64), encoding="utf-8")
    host = subprocess.Popen([str(binary)], cwd=str(root), env=env, stdout=log, stderr=subprocess.STDOUT)
    assert host.wait(timeout=40) != 0
    host = None
    assert sql("SELECT count(*) FROM component_sources WHERE git='%s'" % third_git) == "0"
    assert installation() == manual
    config.write_text("plugins=[]\n", encoding="utf-8")
    launch()
    invoke(source_id)
    print("component bootstrap passed: first install, deferred executor, crash recovery, version trust, live revocation, recovery journal, self-upgrade, rollback, disable, uninstall, batch validation; logs=" + str(root), flush=True)
finally:
    stop()
    log.close()
