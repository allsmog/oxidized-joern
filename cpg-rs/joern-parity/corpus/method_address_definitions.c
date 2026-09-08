int address_defined(int value) { return value; }
int address_declared(int value);
void address_receiver(int (*callback)(int));
void address_distinct(void) {
    int (*first)(int) = address_defined;
    int (*second)(int) = address_declared;
}
void address_overwritten(int value) {
    int (*first)(int) = address_defined;
    first = address_declared;
}
void address_killed(int value) {
    int (*first)(int) = address_defined;
    first = 0;
}
void address_explicit(void) { int (*first)(int) = &address_defined; }
void address_no_parameters() { int (*first)(int) = address_defined; }
void address_returned(int value) { int (*first)(int) = address_defined; return; }
void address_argument(void) { address_receiver(address_defined); }
