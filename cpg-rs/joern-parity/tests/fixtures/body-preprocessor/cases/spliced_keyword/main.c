#define ENABLE 1
# un\
def ENABLE
int choose(int x){
#ifdef ENABLE
return x;
#else
return 0;
#endif
}
