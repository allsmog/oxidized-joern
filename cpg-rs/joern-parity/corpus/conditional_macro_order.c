#define ORDER_STEP(x) ((x) + 1)
int global_before_undef = ORDER_STEP(2);
int before_undef(int value) { return ORDER_STEP(value); }
#undef ORDER_STEP
int after_undef(int value) { return value; }
