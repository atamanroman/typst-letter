// Geschäftsbrief (DIN 5008)
#import "shared/din-letter.typ": din-letter

#show: din-letter.with(
  name: "Erika Musterfrau",
  address: ("Musterstraße 12", "10115 Berlin"),
  contact: (tel: "Tel. +49 30 123456", email: "erika@musterfrau.de"),
  recipient: [
    Beispiel GmbH \
    z. Hd. Herrn Schmidt \
    Industriestraße 5 \
    80331 München
  ],
  place-date: "Berlin, " + datetime.today().display("[day].[month].[year]"),
  subject: [Ihre Anfrage vom 3. Juli 2026],
  salutation: "Sehr geehrter Herr Schmidt,",
  signature: "/shared/signature.png",
)

vielen Dank für Ihre Anfrage. Gerne unterbreiten wir Ihnen das
folgende Angebot. Lorem ipsum dolor sit amet, consectetuer
adipiscing elit, sed diam nonummy nibh euismod tincidunt.

Für Rückfragen stehen wir Ihnen jederzeit gerne zur Verfügung.
