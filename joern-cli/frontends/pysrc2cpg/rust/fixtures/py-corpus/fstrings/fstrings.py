def formatting(name, value):
    return f"{name}={value!r:>{value}}"


def nested(items):
    return f"count={len(items)} first={items[0] if items else 'none'}"
