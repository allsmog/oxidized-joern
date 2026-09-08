#undef __STDC__
#undef __STDC_VERSION__
#undef __STDC_HOSTED__
#define __STDC__ 2
#define __STDC_VERSION__ 201112L
#define __STDC_HOSTED__ 0
long value(void) { return __STDC__ + __STDC_VERSION__ + __STDC_HOSTED__; }
