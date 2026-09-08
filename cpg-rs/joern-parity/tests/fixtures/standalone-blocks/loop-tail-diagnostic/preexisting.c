char *source(void);
void sink(char *value);
void source_before_break_entry(void) {
    char *value = source();
    while (1) {
        break;
        sink(value);
    }
}
void source_before_continue_entry(int count) {
    char *value = source();
    while (count) {
        count--;
        continue;
        sink(value);
    }
}
