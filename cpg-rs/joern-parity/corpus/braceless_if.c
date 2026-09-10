int braceless_sink(int x) { return x; }
int braceless_true(int x) { return x; }
int braceless_false(int x) { return x; }

int braceless_if_call(int x) {
    if (x > 0)
        braceless_sink(x);
    return x;
}

int braceless_if_return(int x) {
    if (x > 0)
        return x;
    return 0;
}

int braceless_if_else(int x) {
    if (x > 0)
        braceless_true(x);
    else
        braceless_false(x);
    return x;
}
