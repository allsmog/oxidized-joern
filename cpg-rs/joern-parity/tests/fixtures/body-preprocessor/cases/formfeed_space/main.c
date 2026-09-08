#define ENABLE 1
#undef ENABLE
int choose(int x){
#ifdef ENABLE
return x;
#else
return 0;
#endif
}
