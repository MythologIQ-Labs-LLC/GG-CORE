"""Missing-auth negatives for packaged typed ONNX acceptance (#171).

A session with no token must never see typed capabilities or run typed
inference. These checks are agnostic to whether the CLI refuses an
unauthenticated listing outright or lists models without capabilities, but
every refusal must carry the CLI's authentication-failure evidence: a
transport error or unrelated protocol failure is never proof of auth.
"""
import json

AUTH_REFUSAL = b"authentication failed"


def auth_refused(result):
    """Non-zero exit whose stderr names the authentication refusal."""
    return result.returncode != 0 and AUTH_REFUSAL in (result.stderr or b"").lower()


class NegativeError(Exception):
    """Stable, non-sensitive failure code."""


def exposes_capabilities(stdout):
    try:
        report = json.loads(stdout)
    except (ValueError, UnicodeError) as exc:
        raise NegativeError("unauthenticated-listing-invalid") from exc
    models = report.get("models") if isinstance(report, dict) else None
    if not isinstance(models, list):
        raise NegativeError("unauthenticated-listing-invalid")
    return any(isinstance(m, dict) and m.get("capabilities") for m in models)


def unauthenticated_refused(invoke, binary, env, embed_id):
    anon = {k: v for k, v in env.items() if k != "CORE_AUTH_TOKEN"}
    listing = invoke(binary, anon, ["models", "list", "--json"], timeout=10)
    if listing.returncode == 0:
        if exposes_capabilities(listing.stdout):
            raise NegativeError("unauthenticated-capabilities-exposed")
    elif not auth_refused(listing):
        raise NegativeError("unauthenticated-listing-refusal-not-auth")
    typed = invoke(binary, anon, ["embed", "--model", embed_id, "--text", "Example."], timeout=30)
    if typed.returncode == 0:
        raise NegativeError("unauthenticated-typed-inference-allowed")
    if not auth_refused(typed):
        raise NegativeError("unauthenticated-typed-refusal-not-auth")
