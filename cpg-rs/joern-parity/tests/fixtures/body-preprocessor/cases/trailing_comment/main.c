#define ENABLE 1
#  undef ENABLE /* trailing */
int choose(int x){
#ifdef ENABLE
return x;
#else
return 0;
#endif
}
