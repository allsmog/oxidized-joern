char *source(void);
char *sanitize(char *value);
void sink(char *value);
char *optional(char *value, int condition) {
    if (condition) { value = "safe"; }
    return value;
}
char *definite(char *value) {
    value = "safe";
    return value;
}
char *loop_optional(char *value, int condition) {
    while (condition) { value = "safe"; condition = 0; }
    return value;
}
char *both_branches(char *value, int condition) {
    if (condition) { value = "safe"; } else { value = "also safe"; }
    return value;
}
char *alternative(char *value, int condition) {
    char *out = "safe";
    if (condition) { out = value; } else { out = "safe"; }
    return out;
}
void optional_entry(int condition) { sink(optional(source(), condition)); }
void definite_entry(void) { sink(definite(source())); }
void loop_entry(int condition) { sink(loop_optional(source(), condition)); }
void both_entry(int condition) { sink(both_branches(source(), condition)); }
void alternative_entry(int condition) { sink(alternative(source(), condition)); }
