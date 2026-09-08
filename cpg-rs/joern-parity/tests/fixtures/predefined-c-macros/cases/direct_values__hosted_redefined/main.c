#undef __STDC_HOSTED__
#define __STDC_HOSTED__ 7
long read_value(int value) { return __STDC_HOSTED__ + value; }
