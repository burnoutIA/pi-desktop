/** Ce qu'affiche le panneau d'aperçu. */
export type Preview = {
  /** « url » : une adresse web (appli lancée sur localhost…) ; sinon un fichier ou dossier local. */
  kind: "url" | "file" | "dir";
  /** L'adresse, ou le chemin absolu du fichier. */
  target: string;
  title: string | null;
  /** Ce que charge le panneau (pour un fichier : son adresse sur le serveur local). */
  url: string;
};

const IMAGE = /\.(png|jpe?g|gif|webp|avif|svg|ico|bmp)$/i;

export const isImage = (path: string) => IMAGE.test(path);

/** Fichiers qu'un clic dans l'arbre ouvre en aperçu plutôt qu'en diff. */
export const isPreviewable = (path: string) => IMAGE.test(path) || /\.(html?|pdf)$/i.test(path);
