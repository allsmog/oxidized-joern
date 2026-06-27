import os
from typing import Iterable, Optional

type Vector[T] = list[T]


def decorate(fn):
    return fn


@decorate
def plain(a, b=1, *args, c, d=2, **kwargs) -> int:
    """A function with positional, default, star and double-star args."""
    return a + b + c + d


def forward(*args, **kwargs):
    # Star / double-star at the *call* site produces Starred + Keyword nodes.
    return plain(*args, c=3, **kwargs)


def generators(xs):
    yield from xs
    for x in xs:
        yield x * 2


def lambdas():
    return lambda value, *rest, key=None: (value, rest, key)


def error_handling(path):
    try:
        with open(path) as handle:
            return handle.read()
    except FileNotFoundError as err:
        raise RuntimeError("missing") from err
    except (OSError, ValueError):
        return None
    finally:
        os.sync()


def identity[T](value: T) -> T:
    return value


def annotated(source: Iterable[int]) -> Optional[int]:
    total = 0
    for item in source:
        total += item
    return total
