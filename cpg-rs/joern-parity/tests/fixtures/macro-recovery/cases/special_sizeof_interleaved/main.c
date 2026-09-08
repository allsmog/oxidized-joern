int global;
int f(void) { return sizeof(int *) + unknown + global + sizeof(int *); }
