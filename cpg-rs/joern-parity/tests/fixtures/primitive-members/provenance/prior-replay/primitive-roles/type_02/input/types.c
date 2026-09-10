unsigned char identity(unsigned char value) { unsigned char local = value; return local; }
unsigned char declared(unsigned char value);
unsigned char call_definition(unsigned char value) { return identity(value); }
unsigned char call_prototype(unsigned char value) { return declared(value); }
