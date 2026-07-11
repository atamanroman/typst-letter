#let letter(
  name: "John Doe",
  address: none,
  contact: (:),
  recipient: [],
  subject: none,
  date: datetime.today().display("[month repr:long] [day], [year]"),
  closing: "Sincerely,",
  signature: none,
  body,
) = {
  set page(paper: "us-letter", margin: (x: 1in, y: 1in))
  set text(font: "Libertinus Serif", size: 11pt, lang: "en")
  set par(justify: true, leading: 0.6em)

  align(right, text(size: 9pt)[
    *#name* \
    #address
    #for (_, v) in contact [ \ #v]
  ])
  v(1.5em)
  recipient
  v(1.5em)
  align(right, date)
  v(1em)
  if subject != none { strong(subject); v(1em) }
  body
  v(1.5em)
  closing
  if signature != none { v(0.2em); image(signature, height: 1.6cm) }
  v(0.2em)
  name
}
