unsigned long long int identity(unsigned long long int value) { unsigned long long int local = value; return local; }
unsigned long long int declared(unsigned long long int value);
unsigned long long int call_definition(unsigned long long int value) { return identity(value); }
unsigned long long int call_prototype(unsigned long long int value) { return declared(value); }
