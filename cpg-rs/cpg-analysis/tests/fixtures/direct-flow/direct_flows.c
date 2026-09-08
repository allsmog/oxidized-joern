char *source(void);
void sink(char *value);
char *identity(char *value) { return value; }
char *replace(char *value) { return "safe"; }

void optional_entry(int condition) {
    char *value = source();
    if (condition) { value = "safe"; }
    sink(value);
}
void definite_entry(void) {
    char *value = source();
    value = "safe";
    sink(value);
}
void both_entry(int condition) {
    char *value = source();
    if (condition) { value = "safe"; } else { value = "also safe"; }
    sink(value);
}
void alternative_entry(int condition) {
    char *value = "safe";
    if (condition) { value = source(); } else { value = "also safe"; }
    sink(value);
}
void reverse_alternative_entry(int condition) {
    char *value = "safe";
    if (condition) { value = "also safe"; } else { value = source(); }
    sink(value);
}
void loop_entry(int condition) {
    char *value = source();
    while (condition) { value = "safe"; condition = 0; }
    sink(value);
}
void loop_carried_entry(int condition) {
    char *value = "safe";
    while (condition) { sink(value); value = source(); condition = 0; }
}
void do_kill_entry(int condition) {
    char *value = source();
    do { value = "safe"; } while (condition);
    sink(value);
}
void for_entry(int condition) {
    char *value = source();
    for (int i = 0; i < condition; i++) { value = "safe"; }
    sink(value);
}
void early_return_entry(int condition) {
    char *value = source();
    if (condition) { value = "safe"; return; }
    sink(value);
}
void sink_before_kill_entry(void) {
    char *value = source();
    sink(value);
    value = "safe";
}
void nested_entry(int condition) {
    char *value = source();
    if (condition) { value = "safe"; }
    sink(identity(identity(value)));
}
void replaced_entry(int condition) {
    char *value = source();
    if (condition) { value = "safe"; }
    sink(replace(value));
}
void join_copy_entry(int condition) {
    char *value = source();
    char *copy = "safe";
    if (condition) { copy = value; } else { copy = "safe"; }
    sink(copy);
}
void branch_kill_entry(int condition) {
    char *value = source();
    if (condition) { value = "safe"; sink(value); }
}
void no_reverse_time_entry(void) {
    char *value = "safe";
    sink(value);
    value = source();
}
void send_optional(char *value, int condition) {
    if (condition) { value = "safe"; }
    sink(value);
}
void handoff_entry(int condition) {
    char *value = source();
    if (condition) { value = "safe"; }
    send_optional(value, condition);
}
void send_kill(char *value) { value = "safe"; sink(value); }
void handoff_kill_entry(void) { send_kill(source()); }
void pointer_entry(char **slot, int condition) {
    *slot = source();
    if (condition) { *slot = "safe"; }
    sink(*slot);
}
struct Box { char *value; };
void member_optional_entry(struct Box *box, int condition) {
    box->value = source();
    if (condition) { box->value = "safe"; }
    sink(box->value);
}
void member_killed_entry(struct Box *box) {
    box->value = source();
    box->value = "safe";
    sink(box->value);
}
void array_entry(int condition) {
    char *values[2];
    values[0] = source();
    if (condition) { values[0] = "safe"; }
    sink(values[0]);
}
void conditional_expr_entry(int condition) { sink(condition ? source() : "safe"); }
void short_circuit_entry(int condition) {
    char *value = source();
    condition && (value = "safe");
    sink(value);
}
void switch_entry(int condition) {
    char *value = source();
    switch (condition) { case 1: value = "safe"; break; }
    sink(value);
}
void switch_killed_entry(int condition) {
    char *value = source();
    switch (condition) { case 1: value = "safe"; break; default: value = "safe"; }
    sink(value);
}
