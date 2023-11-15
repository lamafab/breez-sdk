use crate::{error::SendPaymentError, parse_invoice, LNInvoice, SendPaymentRequest};

pub fn send_payment(req: SendPaymentRequest) -> Result<LNInvoice, SendPaymentError> {
    let parsed_invoice = parse_invoice(req.bolt11.as_str())?;
    let invoice_amount_msat = parsed_invoice.amount_msat.unwrap_or_default();
    let provided_amount_msat = req.amount_msat.unwrap_or_default();

    // Ensure amount is provided for zero invoice
    if provided_amount_msat == 0 && invoice_amount_msat == 0 {
        return Err(SendPaymentError::InvalidAmount {
            err: "Amount must be provided when paying a zero invoice".into(),
        });
    }

    // Ensure amount is not provided for invoice that contains amount
    if provided_amount_msat > 0 && invoice_amount_msat > 0 {
        return Err(SendPaymentError::InvalidAmount {
            err: "Amount should not be provided when paying a non zero invoice".into(),
        });
    }

    Ok(parsed_invoice)
}
