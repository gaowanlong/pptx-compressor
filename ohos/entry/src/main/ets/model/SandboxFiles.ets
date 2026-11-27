import { picker, fileIo as fs, fileUri } from '@kit.CoreFileKit';

/**
 * 鸿蒙的文档选择器返回的是 content:// 形式的 URI，Rust 侧只能读普通路径。
 * 因此先把用户选中的 PPTX 复制到应用沙箱，再把压缩结果导出到用户目录。
 * 这一步是 HAP 沙箱与桌面版「直接拿绝对路径」之间唯一的语义差异。
 */
export class SandboxFiles {
  static readonly INPUT_DIR: string = 'input';
  static readonly OUTPUT_DIR: string = 'output';
  static readonly TEMP_DIR: string = 'tmp';

  static paths(context: Context): string[] {
    return [
      context.filesDir,
      context.cacheDir,
      context.filesDir + '/' + SandboxFiles.INPUT_DIR,
      context.filesDir + '/' + SandboxFiles.OUTPUT_DIR,
      context.cacheDir + '/' + SandboxFiles.TEMP_DIR,
    ];
  }

  static ensureDirs(context: Context): void {
    const dirs: string[] = SandboxFiles.paths(context);
    for (const dir of dirs) {
      try {
        fs.accessSync(dir);
      } catch (err) {
        fs.mkdirSync(dir, true);
      }
    }
  }

  static inputDir(context: Context): string {
    return context.filesDir + '/' + SandboxFiles.INPUT_DIR;
  }

  static outputDir(context: Context): string {
    return context.filesDir + '/' + SandboxFiles.OUTPUT_DIR;
  }

  static tempDir(context: Context): string {
    return context.cacheDir + '/' + SandboxFiles.TEMP_DIR;
  }

  /** 选择 PPTX（可多选），返回沙箱内的真实文件路径。 */
  static async pickAndImport(context: Context): Promise<string[]> {
    const options = new picker.DocumentSelectOptions();
    options.maxSelectNumber = 20;
    options.fileSuffixFilters = ['.pptx'];
    const viewPicker = new picker.DocumentViewPicker(context);
    const uris: string[] = await viewPicker.select(options);

    const result: string[] = [];
    SandboxFiles.ensureDirs(context);
    for (const uri of uris) {
      const name: string = new fileUri.FileUri(uri).name;
      const safeName: string = name.length > 0 ? name : 'document.pptx';
      const dest: string = SandboxFiles.inputDir(context) + '/' + safeName;
      try {
        const srcFile = await fs.open(uri, fs.OpenMode.READ_ONLY);
        const dstFile = await fs.open(
          dest,
          fs.OpenMode.CREATE | fs.OpenMode.WRITE_ONLY | fs.OpenMode.TRUNC,
        );
        await fs.copyFile(srcFile.fd, dstFile.fd);
        await fs.close(srcFile);
        await fs.close(dstFile);
        result.push(dest);
      } catch (err) {
        // 单个文件失败不影响其余文件，与桌面版批量队列的失败隔离一致。
        continue;
      }
    }
    return result;
  }

  /** 把沙箱里的结果导出到用户选择的位置。 */
  static async exportResult(context: Context, localPath: string, fileName: string): Promise<boolean> {
    const options = new picker.DocumentSaveOptions();
    options.newFileNames = [fileName];
    const viewPicker = new picker.DocumentViewPicker(context);
    try {
      const uris: string[] = await viewPicker.save(options);
      if (uris.length === 0) {
        return false;
      }
      const srcFile = await fs.open(localPath, fs.OpenMode.READ_ONLY);
      const dstFile = await fs.open(
        uris[0],
        fs.OpenMode.CREATE | fs.OpenMode.WRITE_ONLY | fs.OpenMode.TRUNC,
      );
      await fs.copyFile(srcFile.fd, dstFile.fd);
      await fs.close(srcFile);
      await fs.close(dstFile);
      return true;
    } catch (err) {
      return false;
    }
  }

  static fileName(path: string): string {
    const parts: string[] = path.split('/');
    return parts.length > 0 ? parts[parts.length - 1] : path;
  }
}
