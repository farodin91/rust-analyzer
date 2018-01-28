use super::*;

pub(super) fn struct_item(p: &mut Parser) {
    assert!(p.at(STRUCT_KW));
    p.bump();

    if !p.expect(IDENT) {
        return;
    }
    type_param_list(p);
    match p.current() {
        WHERE_KW => {
            where_clause(p);
            match p.current() {
                SEMI => {
                    p.bump();
                    return;
                }
                L_CURLY => named_fields(p),
                _ => {
                    //TODO: special case `(` error message
                    p.error().message("expected `;` or `{`").emit();
                    return;
                }
            }
        }
        SEMI => {
            p.bump();
            return;
        }
        L_CURLY => named_fields(p),
        L_PAREN => {
            pos_fields(p);
            p.expect(SEMI);
        }
        _ => {
            p.error().message("expected `;`, `{`, or `(`").emit();
            return;
        }
    }
}

pub(super) fn enum_item(p: &mut Parser) {
    assert!(p.at(ENUM_KW));
    p.bump();
    p.expect(IDENT);
    type_param_list(p);
    where_clause(p);
    if p.expect(L_CURLY) {
        while !p.at(EOF) && !p.at(R_CURLY) {
            let var = p.start();
            attributes::outer_attributes(p);
            if p.at(IDENT) {
                p.bump();
                match p.current() {
                    L_CURLY => named_fields(p),
                    L_PAREN => pos_fields(p),
                    EQ => {
                        p.bump();
                        expressions::expr(p);
                    }
                    _ => (),
                }
                var.complete(p, ENUM_VARIANT);
            } else {
                var.abandon(p);
                p.err_and_bump("expected enum variant");
            }
            if !p.at(R_CURLY) {
                p.expect(COMMA);
            }
        }
        p.expect(R_CURLY);
    }
}

fn named_fields(p: &mut Parser) {
    assert!(p.at(L_CURLY));
    p.bump();
    while !p.at(R_CURLY) && !p.at(EOF) {
        named_field(p);
        if !p.at(R_CURLY) {
            p.expect(COMMA);
        }
    }
    p.expect(R_CURLY);

    fn named_field(p: &mut Parser) {
        let field = p.start();
        visibility(p);
        if p.expect(IDENT) {
            p.expect(COLON);
            types::type_ref(p);
            field.complete(p, NAMED_FIELD);
        } else {
            field.abandon(p);
            p.err_and_bump("expected field declaration");
        }
    }
}

fn pos_fields(p: &mut Parser) {
    if !p.expect(L_PAREN) {
        return;
    }
    while !p.at(R_PAREN) && !p.at(EOF) {
        let pos_field = p.start();
        visibility(p);
        types::type_ref(p);
        pos_field.complete(p, POS_FIELD);

        if !p.at(R_PAREN) {
            p.expect(COMMA);
        }
    }
    p.expect(R_PAREN);
}