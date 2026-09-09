void use(int *value);
void use_char(char *value);
int size(void);
void sized_string(void) {
    char value[2] =
        "x";
    use_char(value);
}
void mixed_array_first(void) {
    int a[2] = {1},
        b[2];
    use(a);
}
void mixed_scalar_first(void) {
    int a=1,
        b[2];
    use(b);
}
void repeat_dimension(void) {
    int a[size()];
    size();
}
void separate_scope(void) {
    { int a[2]={1}; }
    int a[2]={1};
    use(a);
}
