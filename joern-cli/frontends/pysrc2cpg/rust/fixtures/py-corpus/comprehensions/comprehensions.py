def comprehensions(xs):
    squares = [x * x for x in xs if x > 0]
    unique = {x for x in xs}
    mapping = {x: x * x for x in xs}
    lazy = (x for x in xs)
    return squares, unique, mapping, lazy


def walrus(data):
    if (n := len(data)) > 10:
        return n
    return 0
