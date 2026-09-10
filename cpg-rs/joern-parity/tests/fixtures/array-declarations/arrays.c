int global_one[4];
int global_two[2][3];
int global_three[2][3][4];
static int static_global[5];
extern int external_global[6];
int first[2], second[3];
int scalar_before, third[3];
void local_arrays(int n) {
    int local_two[2][3];
    int local_dynamic[n][4];
    static int local_static[5];
    extern int local_external[6];
}
