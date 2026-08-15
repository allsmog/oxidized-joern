def source(value):
    return value


def transform(value):
    return value


def sink(value):
    print(value)


def main(user):
    raw = source(user)
    clean = transform(raw)
    sink(clean)
