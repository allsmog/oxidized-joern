#undef __STDC__
#undef __STDC_VERSION__
#undef __STDC_HOSTED__
long value(void) { return __STDC__ + __STDC_VERSION__ + __STDC_HOSTED__; }
