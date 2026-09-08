#define STANDARD(x) ((__STDC__ + __STDC_VERSION__ + __STDC_HOSTED__) + (x))
long value(int x) { return STANDARD(x); }
