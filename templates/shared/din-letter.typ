// DIN 5008 Form B Geschäftsbrief.
// Maße: Anschriftfeld 45mm von oben / 20mm von links, 85mm × 40mm
// (Fenster­umschlag DL). Falzmarken bei 105mm und 210mm, Lochmarke 148,5mm.
// Informationsblock rechts bei 125mm. Text beginnt bei 98,46mm.

#let din-letter(
  name: "Max Mustermann",
  address: (),               // sender address lines, e.g. ("Musterstr. 1", "12345 Berlin")
  contact: (:),              // e.g. (tel: "+49 30 1234567", email: "max@example.de")
  recipient: [],             // full address block for the window
  return-line: auto,         // small Rücksendeangabe above the recipient
  place-date: auto,          // e.g. "Berlin, 11.07.2026"
  subject: none,
  salutation: "Sehr geehrte Damen und Herren,",
  closing: "Mit freundlichen Grüßen",
  signature: none,
  body,
) = {
  let sender-oneline = (name,) + address.map(str)
  let return-text = if return-line == auto { sender-oneline.join(" · ") } else { return-line }
  let date-text = if place-date == auto {
    datetime.today().display("[day].[month].[year]")
  } else { place-date }

  set page(
    paper: "a4",
    margin: (left: 25mm, right: 20mm, top: 98.46mm, bottom: 25mm),
    background: {
      // Falzmarken + Lochmarke
      place(top + left, dx: 5mm, dy: 105mm, line(length: 4mm, stroke: 0.4pt))
      place(top + left, dx: 5mm, dy: 210mm, line(length: 4mm, stroke: 0.4pt))
      place(top + left, dx: 5mm, dy: 148.5mm, line(length: 6mm, stroke: 0.4pt))

      // Briefkopf
      place(top + left, dx: 25mm, dy: 17mm, text(size: 16pt, weight: "bold", name))

      // Anschriftfeld (85mm × 40mm ab 45mm/20mm)
      place(top + left, dx: 20mm, dy: 45mm, box(width: 85mm, height: 40mm, {
        set text(size: 8pt)
        text(size: 6.5pt, underline(return-text))
        v(4mm)
        set text(size: 11pt)
        recipient
      }))

      // Informationsblock
      place(top + left, dx: 125mm, dy: 50mm, box(width: 65mm, {
        set text(size: 9pt)
        set par(leading: 0.55em)
        strong(name)
        linebreak()
        for l in address { l; linebreak() }
        for (_, v) in contact { v; linebreak() }
        v(2mm)
        date-text
      }))
    },
  )
  set text(font: "Libertinus Serif", size: 11pt, lang: "de")
  set par(justify: true, leading: 0.65em)

  if subject != none {
    strong(subject)
    v(2em)
  }
  salutation
  parbreak()
  body
  v(1.5em)
  closing
  if signature != none { v(0.2em); image(signature, height: 1.4cm) } else { v(3em) }
  name
}
