void render(char *p, int n) {
#if !defined(NO_snprintf) && !defined(NO_vsnprintf)
    (void)snprintf(p, n, "%d", n);
#else
    sprintf(p, "%d", n);
#endif
}
