typedef int *P, **PP;
typedef const int *CP;
P first(void *x){return (P)(x);}
PP second(void *x){return (PP)(x);}
CP third(void *x){return (CP)(x);}
