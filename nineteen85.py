import json
import os
import urllib.error
import urllib.request

BASE = "https://api.1985.sh/v1"


class Error(Exception):
    def __init__(self, status, message):
        super().__init__(message)
        self.status = status


class Client:
    def __init__(self, key=None, base_url=None):
        self.key = key or os.environ.get("NINETEEN85_KEY")
        self.base_url = (base_url or os.environ.get("NINETEEN85_URL") or BASE).rstrip("/")

    def _open(self, method, path, body=None):
        data = None if body is None else json.dumps(body).encode()
        req = urllib.request.Request(self.base_url + path, data=data, method=method)
        req.add_header("content-type", "application/json")
        req.add_header("user-agent", "1985-python")
        if self.key:
            req.add_header("authorization", "Bearer " + self.key)
        try:
            return urllib.request.urlopen(req, timeout=600)
        except urllib.error.HTTPError as e:
            try:
                message = json.loads(e.read())["error"]["message"]
            except Exception:
                message = "error %d" % e.code
            raise Error(e.code, message) from None

    def _json(self, method, path, body=None):
        with self._open(method, path, body) as r:
            return json.loads(r.read() or b"{}")

    def _events(self, path, body):
        with self._open("POST", path, body) as r:
            event = ""
            for raw in r:
                line = raw.decode().rstrip("\r\n")
                if line.startswith("event:"):
                    event = line[6:].strip()
                elif line.startswith("data:"):
                    data = line[5:].strip()
                    if data and data != "[DONE]":
                        o = json.loads(data)
                        if event and isinstance(o, dict):
                            o.setdefault("event", event)
                        yield o

    def run(self, model, input, stream=True, **params):
        body = {"model": model, "input": input, "stream": stream, **params}
        return self._events("/responses", body) if stream else self._json("POST", "/responses", body)

    def text(self, model, input, **params):
        return "".join(e["delta"] for e in self.run(model, input, True, **params) if e.get("type") == "response.output_text.delta")

    def search(self, query):
        return self._json("POST", "/search", {"query": query})["results"]

    def sandbox(self):
        return Sandbox(self, self._json("POST", "/sandboxes", {})["id"])

    def agent(self, goal, model=None):
        return self._events("/agents", {"goal": goal, **({"model": model} if model else {})})

    def models(self):
        return self._json("GET", "/models")["data"]

    def refresh(self):
        self.key = self._json("POST", "/keys", {})["key"]
        return self.key

    def permit(self, address, value, deadline, signature):
        self.key = self._json("POST", "/keys", {"address": address, "value": str(value), "deadline": deadline, "signature": signature})["key"]
        return self.key


class Sandbox:
    def __init__(self, client, id):
        self.client = client
        self.id = id

    def exec(self, command, timeout=60):
        return self.client._json("POST", "/sandboxes/%s/exec" % self.id, {"command": command, "timeout": timeout})

    def close(self):
        self.client._json("DELETE", "/sandboxes/%s" % self.id)

    def __enter__(self):
        return self

    def __exit__(self, *a):
        self.close()
