char *source(void);
void sink(char *value);

void bare_entry(void) {
    { sink(source()); }
}

void nested_entry(void) {
    char *value = "safe";
    { { value = source(); } }
    sink(value);
}

void kill_entry(void) {
    char *value = source();
    { value = "safe"; }
    sink(value);
}

void labeled_entry(void) {
    goto target;
target: { sink(source()); }
}

void switch_entry(int condition) {
    switch (condition) {
    case 1: { sink(source()); break; }
    default: { break; }
    }
}

void loop_block_entry(int condition) {
    while (condition) {
        { sink(source()); }
        condition = 0;
    }
}

void empty_entry(void) {
    { }
    { sink(source()); }
    { }
}

int shadowed(int value);
int block_prototypes(int value) {
    {
        int shadowed(int value);
        shadowed(value);
        (shadowed)(value);
    }
    return shadowed(value) + (shadowed)(value);
}

int callback(int value) { return value; }
int block_pointer_scope(int value) {
    int (*callable)(int) = callback;
    {
        int callable(int value);
        callable(value);
    }
    return callable(value);
}

int block_local_scope(int value) {
    { int value = 1; value = value + 1; }
    return value;
}

char *block_return(char *value) {
    { return value; }
    return "safe";
}

void return_entry(void) {
    sink(block_return(source()));
}

void after_return_entry(void) {
    { return; }
    sink(source());
}

void continue_entry(int count) {
    while (count) {
        { count--; continue; }
        sink(source());
    }
}

void break_block_entry(void) {
    while (1) {
        { break; }
        sink(source());
    }
}

void empty_only(void) { { } }
