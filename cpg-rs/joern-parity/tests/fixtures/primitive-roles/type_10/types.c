signed identity(signed value) { signed local = value; return local; }
signed declared(signed value);
signed call_definition(signed value) { return identity(value); }
signed call_prototype(signed value) { return declared(value); }
